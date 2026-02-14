/// Ollama HTTP API client for the LLM Smart Path.
///
/// Communicates with a local Ollama instance at `localhost:11434` using the
/// synchronous `ureq` HTTP client. Provides:
///
/// - **Health check**: verify Ollama is running and has a model loaded.
/// - **Chat**: send system + user messages and receive condensed output.
///
/// Uses the `/api/chat` endpoint so that Ollama applies the correct chat
/// template tokens for each model (Llama, Qwen, Gemma, Phi, etc.)
/// automatically — we never hard-code `<|start_header_id|>` or similar.
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::config::SmartPathConfig;

// ---------------------------------------------------------------------------
// Request / response types for the Ollama API
// ---------------------------------------------------------------------------

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    /// Build a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    /// Build a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

/// Request body for `POST /api/chat`.
#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    options: ChatOptions,
}

/// Generation options included in the request.
#[derive(Debug, Serialize)]
struct ChatOptions {
    temperature: f32,
    /// Maximum number of tokens in the response.
    num_predict: u32,
    /// Context window size in tokens.
    ///
    /// Ollama auto-expands the context window to fit the prompt, which can
    /// push the KV cache out of VRAM and force CPU inference. Setting this
    /// explicitly keeps the model 100% on GPU for fast inference.
    num_ctx: u32,
}

/// Response body from `POST /api/chat` (non-streaming).
#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatResponseMessage,
    #[allow(dead_code)]
    done: bool,
}

/// The assistant message within a chat response.
#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    #[allow(dead_code)]
    role: String,
    content: String,
}

/// Response body from `GET /api/tags` — lists available models.
#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<ModelEntry>,
}

/// A single model entry returned by the tags endpoint.
#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[allow(dead_code)]
    name: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Synchronous Ollama HTTP client.
///
/// Created from a [`SmartPathConfig`] and reused for the lifetime of a single
/// `terse run` invocation. Not cached across invocations — each hook ↔ run
/// cycle creates a fresh client.
#[derive(Debug)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    timeout: Duration,
}

impl OllamaClient {
    /// Build a client from the resolved config.
    pub fn from_config(config: &SmartPathConfig) -> Self {
        Self {
            base_url: config.ollama_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            timeout: Duration::from_millis(config.timeout_ms),
        }
    }

    /// Check whether Ollama is reachable and has at least one model loaded.
    ///
    /// Uses a short timeout (5 s) so the hook doesn't stall if Ollama is down.
    /// Resolves `localhost` to `127.0.0.1` to avoid IPv6 DNS delays on Windows.
    pub fn is_healthy(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        // On Windows, "localhost" may try IPv6 (::1) first, causing delays
        // when Ollama only binds to IPv4. Use 127.0.0.1 directly.
        let url = url.replace("://localhost", "://127.0.0.1");
        let result = ureq::get(&url).timeout(Duration::from_secs(5)).call();

        match result {
            Ok(resp) => {
                if let Ok(tags) = resp.into_json::<TagsResponse>() {
                    !tags.models.is_empty()
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    /// Send chat messages to Ollama and return the assistant's response.
    ///
    /// Uses the `/api/chat` endpoint with `stream: false`. Ollama applies
    /// the model-specific chat template (Llama `<|start_header_id|>`,
    /// Qwen `<|im_start|>`, Gemma `<start_of_turn>`, Phi `<|system|>`,
    /// etc.) automatically based on the model's metadata.
    ///
    /// Temperature is 0.0 for deterministic condensation. The `num_predict`
    /// budget is set proportional to the total message length, capped at
    /// 2048 tokens.
    pub fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        // On Windows, "localhost" may try IPv6 (::1) first, causing timeouts.
        let url = url.replace("://localhost", "://127.0.0.1");

        let total_len: usize = messages.iter().map(|m| m.content.len()).sum();
        let token_budget = estimate_response_budget(total_len);

        let body = ChatRequest {
            model: &self.model,
            messages,
            stream: false,
            options: ChatOptions {
                temperature: 0.0,
                num_predict: token_budget,
                num_ctx: CONTEXT_WINDOW,
            },
        };

        let resp = ureq::post(&url)
            .timeout(self.timeout)
            .send_json(&body)
            .context("Ollama chat request failed")?;

        let parsed: ChatResponse = resp
            .into_json()
            .context("failed to parse Ollama chat response")?;

        if parsed.message.content.trim().is_empty() {
            anyhow::bail!("Ollama returned an empty response");
        }

        Ok(parsed.message.content)
    }

    /// Send a raw prompt to Ollama and return the generated text.
    ///
    /// Uses `POST /api/generate` with `stream: false`. Prefer [`chat`] for
    /// new code — this method is kept for backward compatibility and testing.
    #[allow(dead_code)]
    pub fn generate(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let url = url.replace("://localhost", "://127.0.0.1");

        let token_budget = estimate_response_budget(prompt.len());

        #[derive(Serialize)]
        struct GenerateRequest<'a> {
            model: &'a str,
            prompt: &'a str,
            stream: bool,
            options: ChatOptions,
        }

        #[derive(Deserialize)]
        struct GenerateResponse {
            response: String,
        }

        let body = GenerateRequest {
            model: &self.model,
            prompt,
            stream: false,
            options: ChatOptions {
                temperature: 0.0,
                num_predict: token_budget,
                num_ctx: CONTEXT_WINDOW,
            },
        };

        let resp = ureq::post(&url)
            .timeout(self.timeout)
            .send_json(&body)
            .context("Ollama generate request failed")?;

        let parsed: GenerateResponse = resp
            .into_json()
            .context("failed to parse Ollama generate response")?;

        if parsed.response.trim().is_empty() {
            anyhow::bail!("Ollama returned an empty response");
        }

        Ok(parsed.response)
    }

    /// Return the model name for logging.
    pub fn model_name(&self) -> &str {
        &self.model
    }
}

/// Context window size for Ollama requests.
///
/// Ollama auto-expands the context window to fit the prompt, which causes
/// the KV cache to overflow VRAM (e.g. 131K context on a 1B model → 14 GB,
/// exceeding a 10 GB RTX 3080). This forces CPU inference and ~60 s latency.
///
/// Setting `num_ctx` explicitly keeps the entire model + KV cache in VRAM.
/// For a 1B q4 model at 16K context: ~700 MB weights + ~500 MB KV cache =
/// ~1.2 GB total — well within a 10 GB GPU.
///
/// The context window is the TOTAL budget for input + output tokens.
/// Our prompts use up to ~5K tokens (16K chars / 4 + template overhead),
/// leaving ~11K tokens for the response — more than enough.
const CONTEXT_WINDOW: u32 = 40_960;

/// Estimate a reasonable token budget for the LLM response.
///
/// We want the response shorter than the input but not so tight that the
/// model truncates mid-sentence. Budget is 50% of estimated input tokens,
/// clamped to [512, 4096]. The 512-token floor ensures enough room to
/// condense even small outputs (e.g. 20 git commits at ~15 tokens each).
fn estimate_response_budget(total_chars: usize) -> u32 {
    let input_tokens = (total_chars / 4) as u32;
    let budget = input_tokens / 2; // 50%
    budget.clamp(1024, 4096)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_budget_medium_input() {
        // 2000 chars → 500 tokens → 50% = 250 → clamped to 1024
        assert_eq!(estimate_response_budget(2000), 1024);
    }

    #[test]
    fn estimate_budget_large_input() {
        // 40000 chars → 10000 tokens → 50% = 5000 → clamped to 4096
        assert_eq!(estimate_response_budget(40000), 4096);
    }

    #[test]
    fn client_from_default_config() {
        let config = SmartPathConfig::default();
        let client = OllamaClient::from_config(&config);
        assert_eq!(client.base_url, "http://localhost:11434");
        assert_eq!(client.model, "llama3.2:1b");
        assert_eq!(client.timeout, Duration::from_millis(30_000));
    }

    #[test]
    fn client_strips_trailing_slash() {
        let mut config = SmartPathConfig::default();
        config.ollama_url = "http://localhost:11434/".to_string();
        let client = OllamaClient::from_config(&config);
        assert_eq!(client.base_url, "http://localhost:11434");
    }
}
