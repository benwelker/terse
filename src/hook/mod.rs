use std::fs::{OpenOptions, create_dir_all};
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::hook::protocol::{HookRequest, HookResponse};
use crate::matching;
use crate::optimizers::OptimizerRegistry;

pub mod protocol;

/// Entry point for `terse hook` — the PreToolUse handler.
///
/// Reads the Claude Code hook JSON from stdin, decides whether the command
/// can be optimized, and writes a JSON response to stdout:
/// - Passthrough (`{}`) for unrecognized or non-Bash commands.
/// - Rewrite (`hookSpecificOutput.updatedInput`) to route the command
///   through `terse run`, which executes and optimizes it.
pub fn run() -> Result<()> {
    log_hook_event("hook invoked");

    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .context("failed reading hook payload from stdin")?;

    let response = match handle_request(&stdin_buf) {
        Ok(response) => response,
        Err(error) => {
            log_hook_event(&format!("hook error: {error}"));
            HookResponse::passthrough()
        }
    };

    let json = serde_json::to_string(&response).context("failed to serialize hook response")?;
    std::io::stdout()
        .write_all(json.as_bytes())
        .context("failed writing hook response to stdout")?;

    Ok(())
}

fn handle_request(raw: &str) -> Result<HookResponse> {
    if raw.trim().is_empty() {
        return Ok(HookResponse::passthrough());
    }

    let request: HookRequest = serde_json::from_str(raw).context("invalid hook request JSON")?;

    let is_bash = request.is_bash();
    let command_preview = summarize_command(request.tool_input.command.as_deref());
    log_hook_event(&format!(
        "request tool={} is_bash={} command=\"{}\"",
        request.tool_name, is_bash, command_preview
    ));

    if !is_bash {
        return Ok(HookResponse::passthrough());
    }

    let Some(command) = request.tool_input.command.as_deref() else {
        return Ok(HookResponse::passthrough());
    };

    // Prevent infinite loop: if the command is already a terse invocation, pass through.
    if matching::is_terse_invocation(command) {
        log_hook_event("command is already a terse invocation; passthrough");
        return Ok(HookResponse::passthrough());
    }

    // Heredocs embed multi-line content inline — never rewrite these.
    if matching::contains_heredoc(command) {
        log_hook_event("command contains heredoc; passthrough");
        return Ok(HookResponse::passthrough());
    }

    let registry = OptimizerRegistry::new();
    if registry.can_handle(command) {
        let rewritten = build_rewrite_command(command)?;
        log_hook_event(&format!("optimizer matched; rewriting to: {rewritten}"));
        return Ok(HookResponse::rewrite(&rewritten));
    }

    log_hook_event("no optimizer matched; passthrough");
    Ok(HookResponse::passthrough())
}

/// Build the rewritten command that routes execution through `terse run`.
///
/// Uses `std::env::current_exe()` to locate the running binary so the
/// rewrite works from both development builds and installed locations.
fn build_rewrite_command(original_command: &str) -> Result<String> {
    let exe = std::env::current_exe().context("failed to determine terse executable path")?;
    let exe_str = exe.display().to_string();

    // Escape any double quotes in the original command
    let escaped = original_command.replace('"', "\\\"");

    Ok(format!("\"{exe_str}\" run \"{escaped}\""))
}

fn summarize_command(command: Option<&str>) -> String {
    let raw = command.unwrap_or("").replace(['\r', '\n'], " ");
    if raw.len() > 200 {
        format!("{}...", &raw[..200])
    } else {
        raw
    }
}

fn log_hook_event(message: &str) {
    let Some(log_path) = hook_log_path() else {
        return;
    };

    if let Some(parent) = log_path.parent() {
        if create_dir_all(parent).is_err() {
            return;
        }
    }

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) else {
        return;
    };

    let _ = writeln!(file, "{} {}", Utc::now().to_rfc3339(), message);
}

fn hook_log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("hook.log"))
}
