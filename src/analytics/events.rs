//! Raw event logger — logs every tool/command received by the hook for research.
//!
//! Unlike the command-log.jsonl (which only records optimized commands that go
//! through `terse run`), this log captures **every** hook invocation including
//! passthrough decisions. This data is used to discover high-frequency
//! commands and tools that could benefit from new optimizers.
//!
//! Log file: `~/.terse/events.jsonl`

use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Event entry
// ---------------------------------------------------------------------------

/// A raw hook event entry. One line per hook invocation.
#[derive(Debug, Serialize)]
pub struct HookEvent {
    pub timestamp: String,
    /// Tool name from hook request (e.g. `"Bash"`, `"Read"`, `"Write"`).
    pub tool_name: String,
    /// The command string (if tool is Bash), or empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Hook decision: `"rewrite"` or `"passthrough"`.
    pub decision: String,
    /// Reason for passthrough (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passthrough_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Log a raw hook event to `~/.terse/events.jsonl`.
///
/// Best-effort — failures are silently ignored.
pub fn log_hook_event(event: &HookEvent) {
    let _ = append_event(event);
}

/// Convenience: log a rewrite event.
pub fn log_rewrite(tool_name: &str, command: Option<&str>) {
    let event = HookEvent {
        timestamp: Utc::now().to_rfc3339(),
        tool_name: tool_name.to_string(),
        command: command.map(|s| s.to_string()),
        decision: "rewrite".to_string(),
        passthrough_reason: None,
    };
    log_hook_event(&event);
}

/// Convenience: log a passthrough event.
pub fn log_passthrough(tool_name: &str, command: Option<&str>, reason: &str) {
    let event = HookEvent {
        timestamp: Utc::now().to_rfc3339(),
        tool_name: tool_name.to_string(),
        command: command.map(|s| s.to_string()),
        decision: "passthrough".to_string(),
        passthrough_reason: Some(reason.to_string()),
    };
    log_hook_event(&event);
}

fn append_event(event: &HookEvent) -> anyhow::Result<()> {
    let Some(path) = events_log_path() else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let json = serde_json::to_string(event)?;
    writeln!(file, "{json}")?;

    Ok(())
}

fn events_log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("events.jsonl"))
}
