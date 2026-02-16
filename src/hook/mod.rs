use std::fs::{OpenOptions, create_dir_all};
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::analytics::events;
use crate::hook::protocol::{
    CopilotHookRequest, CopilotHookResponse, HookRequest, HookResponse, ToolKind,
};
use crate::router::{self, HookDecision};

pub mod protocol;

/// Entry point for `terse hook` — the PreToolUse handler.
///
/// Reads the Claude Code hook JSON from stdin, delegates the routing decision
/// to the [`router`](crate::router), and writes a JSON response to stdout:
/// - Passthrough (`{}`) for commands the router cannot or should not optimize.
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
    let kind = request.tool_kind();

    log_hook_event(&format!(
        "request tool={} kind={} command=\"{}\"",
        request.tool_name,
        kind,
        summarize_command(request.tool_input.command.as_deref()),
    ));

    match kind {
        ToolKind::Bash => handle_bash(&request),
        ToolKind::Unsupported => {
            events::log_passthrough(
                &request.tool_name,
                request.tool_input.command.as_deref(),
                "unsupported tool",
            );
            Ok(HookResponse::passthrough())
        }
    }
}

// ---------------------------------------------------------------------------
// Per-tool handlers
// ---------------------------------------------------------------------------

/// Handle a Bash tool invocation — route through the optimizer pipeline.
fn handle_bash(request: &HookRequest) -> Result<HookResponse> {
    let Some(command) = request.tool_input.command.as_deref() else {
        events::log_passthrough(&request.tool_name, None, "no command");
        return Ok(HookResponse::passthrough());
    };

    let decision = router::decide_hook(command);

    match &decision {
        HookDecision::Rewrite => {
            let rewritten = build_rewrite_command(command)?;
            log_hook_event(&format!("router decided rewrite; command: {rewritten}"));
            events::log_rewrite(&request.tool_name, Some(command));
            Ok(HookResponse::rewrite(&rewritten))
        }
        HookDecision::Passthrough(reason) => {
            log_hook_event(&format!("router decided passthrough ({reason})"));
            events::log_passthrough(&request.tool_name, Some(command), &reason.to_string());
            Ok(HookResponse::passthrough())
        }
    }
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

    if let Some(parent) = log_path.parent()
        && create_dir_all(parent).is_err()
    {
        return;
    }

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) else {
        return;
    };

    let _ = writeln!(file, "{} {}", Utc::now().to_rfc3339(), message);
}

fn hook_log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("hook.log"))
}

// ---------------------------------------------------------------------------
// Copilot hook handler
// ---------------------------------------------------------------------------

/// Entry point for `terse copilot-hook` — the Copilot preToolUse handler.
///
/// Reads the Copilot hook JSON from stdin, routes optimizable commands
/// through `terse run` via `hookSpecificOutput.updatedInput`, and writes
/// a JSON response to stdout.
///
/// The response includes both a top-level `permissionDecision: "allow"`
/// (standard Copilot format) **and** `hookSpecificOutput.updatedInput`
/// (Claude Code rewrite protocol). If the Copilot runtime processes
/// `hookSpecificOutput`, commands are transparently optimized. If not,
/// the top-level allow decision ensures graceful passthrough.
pub fn run_copilot() -> Result<()> {
    log_hook_event("copilot-hook invoked");

    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .context("failed reading copilot hook payload from stdin")?;

    let response = match handle_copilot_request(&stdin_buf) {
        Ok(response) => response,
        Err(error) => {
            log_hook_event(&format!("copilot-hook error: {error}"));
            // Golden rule: any error → graceful passthrough (allow).
            CopilotHookResponse::allow()
        }
    };

    let json =
        serde_json::to_string(&response).context("failed to serialize copilot hook response")?;
    std::io::stdout()
        .write_all(json.as_bytes())
        .context("failed writing copilot hook response to stdout")?;

    Ok(())
}

fn handle_copilot_request(raw: &str) -> Result<CopilotHookResponse> {
    if raw.trim().is_empty() {
        return Ok(CopilotHookResponse::allow());
    }

    let request: CopilotHookRequest =
        serde_json::from_str(raw).context("invalid copilot hook request JSON")?;
    let kind = request.tool_kind();
    let command = request.command();

    log_hook_event(&format!(
        "copilot request tool={} kind={} command=\"{}\"",
        request.tool_name,
        kind,
        summarize_command(command.as_deref()),
    ));

    match kind {
        ToolKind::Bash => handle_copilot_bash(&request, command.as_deref()),
        ToolKind::Unsupported => {
            events::log_passthrough(
                &request.tool_name,
                command.as_deref(),
                "unsupported copilot tool",
            );
            Ok(CopilotHookResponse::allow())
        }
    }
}

/// Handle a Copilot Bash tool invocation — route through the optimizer pipeline.
///
/// Uses the same routing logic as Claude Code hooks. Optimizable commands are
/// rewritten to `terse run "<cmd>"` via `hookSpecificOutput.updatedInput`.
/// The top-level `permissionDecision: "allow"` ensures safe fallback if
/// the Copilot runtime does not process the rewrite.
fn handle_copilot_bash(
    request: &CopilotHookRequest,
    command: Option<&str>,
) -> Result<CopilotHookResponse> {
    let Some(command) = command else {
        events::log_passthrough(&request.tool_name, None, "no command in copilot bash");
        return Ok(CopilotHookResponse::allow());
    };

    let decision = router::decide_hook(command);

    match &decision {
        HookDecision::Rewrite => {
            let rewritten = build_rewrite_command(command)?;
            log_hook_event(&format!("copilot: rewriting command: {rewritten}"));
            events::log_rewrite(&request.tool_name, Some(command));
            Ok(CopilotHookResponse::rewrite(&rewritten))
        }
        HookDecision::Passthrough(reason) => {
            log_hook_event(&format!("copilot: passthrough ({reason})"));
            events::log_passthrough(&request.tool_name, Some(command), &reason.to_string());
            Ok(CopilotHookResponse::allow())
        }
    }
}
