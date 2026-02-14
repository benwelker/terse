use std::fs::{self, OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Command log entry (JSONL analytics)
// ---------------------------------------------------------------------------

/// A single entry in the structured analytics log (`~/.terse/command-log.jsonl`).
///
/// Each entry records the result of an optimized command execution, including
/// token counts, path selection, and timing. Used by the reporter for
/// aggregation and `terse stats` / `terse analyze` / `terse discover`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogEntry {
    pub timestamp: String,
    pub command: String,
    /// Optimization path taken: `"fast"`, `"smart"`, or `"passthrough"`.
    #[serde(default)]
    pub path: String,
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub savings_pct: f64,
    pub optimizer_used: String,
    /// Whether the optimization completed successfully.
    #[serde(default = "default_true")]
    pub success: bool,
    /// LLM latency in milliseconds (only set for smart-path calls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Bytes removed by preprocessing (only set for smart-path calls).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub preprocessing_bytes_removed: Option<usize>,
    /// Percentage of bytes removed by preprocessing (only set for smart-path calls).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub preprocessing_pct: Option<f64>,
    /// Wall-clock time spent in the preprocessing pipeline (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub preprocessing_duration_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// Extract the base command name from a full command string.
///
/// Returns the first whitespace-delimited token (e.g., `"git"` from
/// `"git status --short"`). Used for grouping in analytics.
pub fn base_command_name(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or(command)
}

// ---------------------------------------------------------------------------
// Logging functions
// ---------------------------------------------------------------------------

/// Log a command result with optional LLM latency.
#[allow(dead_code)]
pub fn log_command_result(
    command: &str,
    path: &str,
    original_tokens: usize,
    optimized_tokens: usize,
    optimizer_used: &str,
    success: bool,
) {
    log_command_result_full(
        command,
        path,
        original_tokens,
        optimized_tokens,
        optimizer_used,
        success,
        None,
        None,
        None,
        None,
    )
}

/// Log a command result including LLM latency.
#[allow(dead_code)]
pub fn log_command_result_with_latency(
    command: &str,
    path: &str,
    original_tokens: usize,
    optimized_tokens: usize,
    optimizer_used: &str,
    success: bool,
    latency_ms: Option<u64>,
) {
    log_command_result_full(
        command,
        path,
        original_tokens,
        optimized_tokens,
        optimizer_used,
        success,
        latency_ms,
        None,
        None,
        None,
    )
}

/// Log a command result with all fields including preprocessing metadata.
#[allow(clippy::too_many_arguments)]
pub fn log_command_result_full(
    command: &str,
    path: &str,
    original_tokens: usize,
    optimized_tokens: usize,
    optimizer_used: &str,
    success: bool,
    latency_ms: Option<u64>,
    preprocessing_bytes_removed: Option<usize>,
    preprocessing_pct: Option<f64>,
    preprocessing_duration_ms: Option<u64>,
) {
    let savings_pct = if original_tokens == 0 {
        0.0
    } else {
        ((original_tokens.saturating_sub(optimized_tokens)) as f64 / original_tokens as f64) * 100.0
    };

    let entry = CommandLogEntry {
        timestamp: Utc::now().to_rfc3339(),
        command: command.to_string(),
        path: path.to_string(),
        original_tokens,
        optimized_tokens,
        savings_pct,
        optimizer_used: optimizer_used.to_string(),
        success,
        latency_ms,
        preprocessing_bytes_removed,
        preprocessing_pct,
        preprocessing_duration_ms,
    };

    let _ = append_log_entry(&entry);
}

// ---------------------------------------------------------------------------
// Reading log entries
// ---------------------------------------------------------------------------

/// Read all command log entries from `~/.terse/command-log.jsonl`.
///
/// Silently skips malformed lines. Returns an empty vec if the file does not
/// exist or cannot be read.
pub fn read_all_entries() -> Vec<CommandLogEntry> {
    let Some(path) = command_log_path() else {
        return Vec::new();
    };

    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };

    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<CommandLogEntry>(&line).ok())
        .collect()
}

/// Read log entries filtered to a time window (last N days).
///
/// If `days` is `None`, returns all entries.
pub fn read_entries_since_days(days: Option<u32>) -> Vec<CommandLogEntry> {
    let entries = read_all_entries();

    let Some(days) = days else {
        return entries;
    };

    let cutoff = Utc::now() - chrono::Duration::days(i64::from(days));
    let cutoff_str = cutoff.to_rfc3339();

    entries
        .into_iter()
        .filter(|e| e.timestamp >= cutoff_str)
        .collect()
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

fn append_log_entry(entry: &CommandLogEntry) -> Result<()> {
    let Some(path) = command_log_path() else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let json = serde_json::to_string(entry)?;
    writeln!(file, "{json}")?;

    Ok(())
}

/// Return the path to the command log file.
pub fn command_log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("command-log.jsonl"))
}
