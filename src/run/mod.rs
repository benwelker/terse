use std::io::Write;

use anyhow::{Context, Result};

use crate::analytics::logger::log_command_result_with_latency;
use crate::router;

/// Execute a command with optimization and print the result to stdout.
///
/// This is the entry point for `terse run "command"`. When the PreToolUse hook
/// rewrites a command to `terse run "original_command"`, Claude Code executes
/// terse as a subprocess. This function delegates to the router's execution
/// pipeline and handles I/O:
///
/// 1. Calls [`router::execute_run`] which tries fast path → smart path → passthrough
/// 2. Logs token analytics to `~/.terse/command-log.jsonl`
/// 3. Prints the optimized result to stdout (which Claude sees as the command output)
pub fn execute(command: &str) -> Result<()> {
    let result = router::execute_run(command)?;

    log_command_result_with_latency(
        command,
        result.original_tokens,
        result.optimized_tokens,
        &result.optimizer_name,
        result.latency_ms,
    );

    std::io::stdout()
        .write_all(result.output.as_bytes())
        .context("failed writing output to stdout")?;

    if !result.stderr.is_empty() {
        std::io::stderr()
            .write_all(result.stderr.as_bytes())
            .context("failed writing stderr output")?;
    }

    Ok(())
}
