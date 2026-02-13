/// Ollama HTTP API client for the LLM Smart Path.
///
/// Communicates with a local Ollama instance at `localhost:11434` using the
/// synchronous `ureq` HTTP client. Provides:
///
/// - **Health check**: verify Ollama is running and has a model loaded.
/// - **Generate**: send a prompt and receive condensed output.
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::config::SmartPathConfig;

// ---------------------------------------------------------------------------
// Request / response types for the Ollama API
// ---------------------------------------------------------------------------

/// Request body for `POST /api/generate`.
#[derive(Debug, Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: GenerateOptions,
}

/// Generation options included in the request.
#[derive(Debug, Serialize)]
struct GenerateOptions {
    temperature: f32,
    /// Maximum number of tokens in the response.
    num_predict: u32,
}

/// Response body from `POST /api/generate` (non-streaming).
#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
    #[allow(dead_code)]
    done: bool,
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
        let result = ureq::get(&url)
            .timeout(Duration::from_secs(5))
            .call();

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

    /// Send a prompt to Ollama and return the generated text.
    ///
    /// Uses `stream: false` for simplicity — the full response is returned in
    /// a single JSON payload. Temperature is 0.0 for deterministic condensation.
    ///
    /// The `num_predict` budget is set proportional to the input length,
    /// capped at 2048 tokens — we want the response shorter than the original.
    pub fn generate(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        // On Windows, "localhost" may try IPv6 (::1) first, causing timeouts.
        let url = url.replace("://localhost", "://127.0.0.1");

        let token_budget = estimate_response_budget(prompt);

        let body = GenerateRequest {
            model: &self.model,
            prompt,
            stream: false,
            options: GenerateOptions {
                temperature: 0.0,
                num_predict: token_budget,
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

/// Estimate a reasonable token budget for the LLM response.
///
/// We want the response to be substantially shorter than the input. The budget
/// is approximately 40% of the estimated input tokens, clamped to [64, 2048].
fn estimate_response_budget(prompt: &str) -> u32 {
    let input_tokens = (prompt.len() / 4) as u32;
    let budget = input_tokens * 2 / 5; // ~40%
    budget.clamp(64, 2048)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_budget_small_input() {
        // 100 chars → ~25 tokens → 40% = 10 → clamped to 64
        assert_eq!(estimate_response_budget(&"x".repeat(100)), 64);
    }

    #[test]
    fn estimate_budget_medium_input() {
        // 2000 chars → 500 tokens → 40% = 200
        assert_eq!(estimate_response_budget(&"x".repeat(2000)), 200);
    }

    #[test]
    fn estimate_budget_large_input() {
        // 40000 chars → 10000 tokens → 40% = 4000 → clamped to 2048
        assert_eq!(estimate_response_budget(&"x".repeat(40000)), 2048);
    }

    #[test]
    fn client_from_default_config() {
        let config = SmartPathConfig::default();
        let client = OllamaClient::from_config(&config);
        assert_eq!(client.base_url, "http://localhost:11434");
        assert_eq!(client.model, "llama3.2:1b");
        assert_eq!(client.timeout, Duration::from_millis(5000));
    }

    #[test]
    fn client_strips_trailing_slash() {
        let mut config = SmartPathConfig::default();
        config.ollama_url = "http://localhost:11434/".to_string();
        let client = OllamaClient::from_config(&config);
        assert_eq!(client.base_url, "http://localhost:11434");
    }
}
