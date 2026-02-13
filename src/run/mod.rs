use std::io::Write;

use anyhow::{Context, Result};

use crate::analytics::logger::{log_command_result, log_command_result_with_latency};
use crate::llm;
use crate::llm::config::SmartPathConfig;
use crate::optimizers::OptimizerRegistry;
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

/// Execute a command with optimization and print the result to stdout.
///
/// This is the entry point for `terse run "command"`. When the PreToolUse hook
/// rewrites a command to `terse run "original_command"`, Claude Code executes
/// terse as a subprocess. This function:
///
/// 1. Looks up a matching rule-based optimizer for the command (fast path)
/// 2. If no optimizer matches, runs the command raw, then checks whether the
///    LLM smart path should be used based on output size and configuration
/// 3. Logs token analytics to `~/.terse/command-log.jsonl`
/// 4. Prints the optimized result to stdout (which Claude sees as the command output)
///
/// If no optimizer matches and the smart path is disabled or output is too
/// small, the command output is passed through unchanged.
pub fn execute(command: &str) -> Result<()> {
    let registry = OptimizerRegistry::new();

    // --- Fast path: rule-based optimizer ---
    if let Some(result) = registry.execute_first(command) {
        log_command_result(
            command,
            result.original_tokens,
            result.optimized_tokens,
            &result.optimizer_used,
        );

        std::io::stdout()
            .write_all(result.output.as_bytes())
            .context("failed writing optimized output to stdout")?;

        return Ok(());
    }

    // --- No optimizer matched: run the original command ---
    let output = run_shell_command(command)?;
    let raw_text = combine_stdout_stderr(&output.stdout, &output.stderr);
    let raw_tokens = estimate_tokens(&raw_text);

    // --- Smart path: attempt LLM optimization if enabled and worthwhile ---
    let smart_config = SmartPathConfig::load();
    if smart_config.enabled && raw_text.len() >= smart_config.min_output_chars {
        match llm::optimize_with_llm(command, &raw_text) {
            Ok(llm_result) => {
                log_command_result_with_latency(
                    command,
                    llm_result.original_tokens,
                    llm_result.optimized_tokens,
                    &format!("llm:{}", llm_result.model),
                    Some(llm_result.latency_ms),
                );

                std::io::stdout()
                    .write_all(llm_result.output.as_bytes())
                    .context("failed writing LLM-optimized output to stdout")?;

                return Ok(());
            }
            Err(_) => {
                // LLM failed or validation rejected — fall through to passthrough
            }
        }
    }

    // --- Passthrough: output the raw command result unchanged ---
    log_command_result(command, raw_tokens, raw_tokens, "passthrough");

    std::io::stdout()
        .write_all(output.stdout.as_bytes())
        .context("failed writing command output to stdout")?;

    if !output.stderr.is_empty() {
        std::io::stderr()
            .write_all(output.stderr.as_bytes())
            .context("failed writing stderr output")?;
    }

    Ok(())
}

/// Combine stdout and stderr into a single string for token counting.
fn combine_stdout_stderr(stdout: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    }
}
