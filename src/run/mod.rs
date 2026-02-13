use std::io::Write;

use anyhow::{Context, Result};

use crate::analytics::logger::log_command_result;
use crate::optimizers::OptimizerRegistry;
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

/// Execute a command with optimization and print the result to stdout.
///
/// This is the entry point for `terse run "command"`. When the PreToolUse hook
/// rewrites a command to `terse run "original_command"`, Claude Code executes
/// terse as a subprocess. This function:
///
/// 1. Looks up a matching optimizer for the command
/// 2. Runs the command (the optimizer may run a substituted command instead)
/// 3. Logs token analytics to `~/.terse/command-log.jsonl`
/// 4. Prints the optimized result to stdout (which Claude sees as the command output)
///
/// If no optimizer matches, the command is executed as-is and passed through.
pub fn execute(command: &str) -> Result<()> {
    let registry = OptimizerRegistry::new();

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

    // No optimizer matched — run the original command and pass through unchanged
    let output = run_shell_command(command)?;
    let tokens = estimate_tokens(&output.stdout);

    log_command_result(command, tokens, tokens, "passthrough");

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
