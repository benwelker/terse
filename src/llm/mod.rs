/// LLM Smart Path — local LLM optimization via Ollama HTTP API.
///
/// This module provides the second optimization path in TERSE's dual-path
/// architecture. When no rule-based optimizer can handle a command, the smart
/// path sends the raw output to a local Ollama instance for intelligent
/// condensation.
///
/// # Feature Flag
///
/// The smart path is **disabled by default** and must be explicitly enabled:
///
/// - Environment variable: `TERSE_SMART_PATH=1`
/// - JSON config file: `~/.terse/config.json` → `{ "smart_path": { "enabled": true } }`
///
/// See [`config::SmartPathConfig`] for full configuration options.
///
/// # Architecture
///
/// The smart path participates at two levels of the execution model:
///
/// 1. **Hook level** (`terse hook`): if the smart path is enabled and Ollama
///    is healthy, the hook rewrites unoptimized commands to `terse run` so
///    they are routed through TERSE even without a rule-based optimizer.
///
/// 2. **Run level** (`terse run`): after executing the command and capturing
///    output, if the output exceeds `min_output_chars` the smart path sends
///    it to the LLM for condensation.
use std::time::Instant;

use anyhow::Result;

pub mod config;
pub mod ollama;
pub mod prompts;
pub mod validation;

use config::SmartPathConfig;
use ollama::OllamaClient;
use prompts::build_prompt;
use validation::validate_llm_output;

/// Result of an LLM optimization attempt.
#[derive(Debug, Clone)]
pub struct LlmResult {
    /// The condensed output text.
    pub output: String,
    /// Token estimate of the original raw output.
    pub original_tokens: usize,
    /// Token estimate of the condensed output.
    pub optimized_tokens: usize,
    /// Model name used for generation.
    pub model: String,
    /// Latency of the LLM call in milliseconds.
    pub latency_ms: u64,
    /// Command category detected for prompt selection.
    #[allow(dead_code)]
    pub category: String,
}

/// Check whether the LLM smart path is available for use by the hook.
///
/// Returns `true` if:
/// 1. The feature flag is enabled (env var or config file).
/// 2. Ollama is reachable and has at least one model loaded.
///
/// This is called from the hook to decide whether to rewrite commands that
/// have no rule-based optimizer.
pub fn is_smart_path_available() -> bool {
    let config = SmartPathConfig::load();
    if !config.enabled {
        return false;
    }

    let client = OllamaClient::from_config(&config);
    client.is_healthy()
}

/// Attempt to optimize raw command output via the LLM smart path.
///
/// Called from `terse run` when:
/// - No rule-based optimizer matched the command
/// - The raw output exceeds `min_output_chars`
/// - The smart path feature flag is enabled
///
/// Returns `Ok(LlmResult)` on success, or `Err` if the LLM call fails or
/// validation rejects the response.
pub fn optimize_with_llm(command: &str, raw_output: &str) -> Result<LlmResult> {
    let config = SmartPathConfig::load();

    // Double-check the feature flag (caller should have checked, but be safe)
    if !config.enabled {
        anyhow::bail!("smart path is disabled");
    }

    let client = OllamaClient::from_config(&config);
    let category = prompts::classify_command(command);

    let prompt = build_prompt(command, raw_output);

    let start = Instant::now();
    let llm_output = client.generate(&prompt)?;
    let latency_ms = start.elapsed().as_millis() as u64;

    // Validate before accepting
    validate_llm_output(raw_output, &llm_output)?;

    let original_tokens = crate::utils::token_counter::estimate_tokens(raw_output);
    let optimized_tokens = crate::utils::token_counter::estimate_tokens(&llm_output);

    Ok(LlmResult {
        output: llm_output.trim().to_string(),
        original_tokens,
        optimized_tokens,
        model: client.model_name().to_string(),
        latency_ms,
        category: category.to_string(),
    })
}
