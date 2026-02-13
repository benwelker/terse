use anyhow::{Result, anyhow};

use crate::optimizers::{OptimizedOutput, Optimizer};
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

pub struct GitOptimizer;

impl GitOptimizer {
    pub fn new() -> Self {
        Self
    }
}

impl Optimizer for GitOptimizer {
    fn name(&self) -> &'static str {
        "git"
    }

    fn can_handle(&self, command: &str) -> bool {
        let Some(normalized) = normalized_git_command(command) else {
            return false;
        };

        normalized.starts_with("git status")
            || normalized.starts_with("git log")
            || normalized.starts_with("git diff")
            || normalized.starts_with("git branch")
            || is_short_status_command(&normalized)
    }

    fn execute_and_optimize(&self, command: &str) -> Result<OptimizedOutput> {
        let normalized =
            normalized_git_command(command).ok_or_else(|| anyhow!("not a git command"))?;

        // Run the original command to capture raw output for token counting.
        // Even for command substitution optimizers, we need the original output
        // to calculate accurate token savings.
        let raw = run_shell_command(command)?;
        let raw_text = combine_output(&raw.stdout, &raw.stderr);
        let original_tokens = estimate_tokens(&raw_text);

        let optimized = if normalized.starts_with("git status") {
            optimize_with_substitution(command, "git status", "git status --short --branch")?
        } else if normalized.starts_with("git log") {
            optimize_with_substitution(command, "git log", "git log --oneline -n 20")?
        } else if normalized.starts_with("git diff") {
            compact_git_diff(&raw_text)
        } else if is_short_status_command(&normalized) {
            summarize_git_operation(&normalized, &raw_text)
        } else if normalized.starts_with("git branch") {
            compact_git_branches(&raw_text)
        } else {
            return Err(anyhow!("git command not supported by optimizer"));
        };

        Ok(OptimizedOutput {
            original_tokens,
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

fn optimize_with_substitution(command: &str, from: &str, to: &str) -> Result<String> {
    let substituted = command.replacen(from, to, 1);
    let output = run_shell_command(&substituted)?;

    let mut combined = output.stdout;
    if !output.stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(output.stderr.trim_end());
    }

    Ok(combined)
}

fn combine_output(stdout: &str, stderr: &str) -> String {
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn normalized_git_command(command: &str) -> Option<String> {
    let mut normalized = command.trim();

    if let Some((_, tail)) = normalized.rsplit_once("&&") {
        normalized = tail.trim();
    }

    if let Some((_, tail)) = normalized.rsplit_once(';') {
        normalized = tail.trim();
    }

    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("git ") {
        Some(lower)
    } else {
        None
    }
}

fn is_short_status_command(command: &str) -> bool {
    command.starts_with("git push")
        || command.starts_with("git pull")
        || command.starts_with("git fetch")
        || command.starts_with("git add")
        || command.starts_with("git commit")
}

fn summarize_git_operation(command: &str, raw_output: &str) -> String {
    let has_error = raw_output.to_ascii_lowercase().contains("error");
    let action = command
        .split_whitespace()
        .nth(1)
        .unwrap_or("operation")
        .to_string();

    if has_error {
        let first_line = raw_output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(raw_output);
        format!("git {action}: failed - {}", first_line.trim())
    } else {
        format!("git {action}: success")
    }
}

fn compact_git_branches(raw_output: &str) -> String {
    let mut current_branch = None;
    let mut others = Vec::new();

    for line in raw_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('*') {
            current_branch = Some(rest.trim().to_string());
        } else {
            others.push(trimmed.to_string());
        }
    }

    let mut output = String::new();
    if let Some(current) = current_branch {
        output.push_str(&format!("* {current}\n"));
    }

    for branch in others {
        output.push_str(&branch);
        output.push('\n');
    }

    output.trim_end().to_string()
}

fn compact_git_diff(raw_output: &str) -> String {
    let mut kept = Vec::new();
    let mut hunk_lines = 0usize;

    for line in raw_output.lines() {
        let should_keep = line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("@@ ")
            || line.starts_with('+')
            || line.starts_with('-');

        if !should_keep {
            continue;
        }

        if line.starts_with('+') || line.starts_with('-') {
            hunk_lines += 1;
            if hunk_lines > 80 {
                continue;
            }
        }

        kept.push(line);

        if kept.len() >= 200 {
            break;
        }
    }

    if kept.is_empty() {
        return raw_output.to_string();
    }

    if raw_output.lines().count() > kept.len() {
        kept.push("...diff truncated...");
    }

    kept.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_git_with_command_prefix() {
        assert!(GitOptimizer::new().can_handle("cd /repo && git status"));
        assert!(GitOptimizer::new().can_handle("git diff"));
        assert!(!GitOptimizer::new().can_handle("npm test"));
    }

    #[test]
    fn compacts_branch_output() {
        let raw = "  main\n* feature/new-api\n  release\n";
        let compact = compact_git_branches(raw);

        assert!(compact.starts_with("* feature/new-api"));
        assert!(compact.contains("main"));
        assert!(compact.contains("release"));
    }

    #[test]
    fn compacts_diff_output() {
        let raw = "diff --git a/src/main.rs b/src/main.rs\nindex 111..222 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n- println!(\"a\");\n+ println!(\"b\");\n }\n";
        let compact = compact_git_diff(raw);

        assert!(compact.contains("diff --git"));
        assert!(compact.contains("@@ -1,3 +1,3 @@"));
        assert!(compact.contains("+ println!(\"b\");"));
    }

    #[test]
    fn summarizes_push_result() {
        let summary = summarize_git_operation("git push", "Everything up-to-date\n");
        assert_eq!(summary, "git push: success");

        let failed = summarize_git_operation("git pull", "error: could not fetch\n");
        assert!(failed.contains("failed"));
    }
}
