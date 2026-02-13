use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandLogEntry {
    pub timestamp: String,
    pub command: String,
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub savings_pct: f64,
    pub optimizer_used: String,
}

pub fn log_command_result(
    command: &str,
    original_tokens: usize,
    optimized_tokens: usize,
    optimizer_used: &str,
) {
    let savings_pct = if original_tokens == 0 {
        0.0
    } else {
        ((original_tokens.saturating_sub(optimized_tokens)) as f64 / original_tokens as f64) * 100.0
    };

    let entry = CommandLogEntry {
        timestamp: Utc::now().to_rfc3339(),
        command: command.to_string(),
        original_tokens,
        optimized_tokens,
        savings_pct,
        optimizer_used: optimizer_used.to_string(),
    };

    let _ = append_log_entry(&entry);
}

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

fn command_log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".terse").join("command-log.jsonl"))
}
