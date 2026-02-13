use std::fs::{OpenOptions, create_dir_all};
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::hook::protocol::{HookRequest, HookResponse};

pub mod protocol;

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

    log_hook_event("hook response=passthrough");

    Ok(())
}

fn handle_request(raw: &str) -> Result<HookResponse> {
    if raw.trim().is_empty() {
        return Ok(HookResponse::passthrough());
    }

    let request: HookRequest =
        serde_json::from_str(raw).context("invalid hook request JSON")?;

    let is_bash = request.is_bash();
    let command = summarize_command(request.tool_input.command.as_deref());
    log_hook_event(&format!(
        "request tool={} is_bash={} command=\"{}\"",
        request.tool_name, is_bash, command
    ));

    Ok(HookResponse::passthrough())
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
