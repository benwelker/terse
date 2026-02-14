use anyhow::{Result, anyhow};

use crate::config::schema::GitOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Subcommand classification
// ---------------------------------------------------------------------------

/// Recognized git subcommands that TERSE can optimize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitSubcommand {
    Status,
    Log,
    Diff,
    Branch,
    Show,
    Stash,
    Worktree,
    /// Push, pull, fetch, add, commit — short success/failure summaries.
    ShortStatus,
}

/// Classify the core command (already lowercased) into a [`GitSubcommand`].
///
/// Returns `None` if the command isn't a recognized git subcommand.
fn classify(lower: &str) -> Option<GitSubcommand> {
    if !lower.starts_with("git ") {
        return None;
    }

    if lower.starts_with("git status") {
        Some(GitSubcommand::Status)
    } else if lower.starts_with("git log") {
        Some(GitSubcommand::Log)
    } else if lower.starts_with("git diff") {
        Some(GitSubcommand::Diff)
    } else if lower.starts_with("git branch") {
        Some(GitSubcommand::Branch)
    } else if lower.starts_with("git show") {
        Some(GitSubcommand::Show)
    } else if lower.starts_with("git stash") {
        Some(GitSubcommand::Stash)
    } else if lower.starts_with("git worktree") {
        Some(GitSubcommand::Worktree)
    } else if lower.starts_with("git push")
        || lower.starts_with("git pull")
        || lower.starts_with("git fetch")
        || lower.starts_with("git add")
        || lower.starts_with("git commit")
    {
        Some(GitSubcommand::ShortStatus)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Argument inspection helpers
// ---------------------------------------------------------------------------

/// Check if any whitespace-delimited token matches a flag exactly, or starts
/// with `flag=` for long options like `--pretty=format:...`.
fn has_flag(text: &str, flags: &[&str]) -> bool {
    text.split_whitespace().any(|word| {
        flags
            .iter()
            .any(|&f| word == f || (f.starts_with("--") && word.starts_with(&format!("{f}="))))
    })
}

/// Check if the command contains a numeric limit argument like `-10` or `-n`.
fn has_numeric_limit(text: &str) -> bool {
    text.split_whitespace().any(|arg| {
        arg == "-n"
            || (arg.starts_with('-')
                && arg.len() > 1
                && arg.as_bytes()[1].is_ascii_digit()
                && arg[1..].bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Extract the stash sub-subcommand (list, show, pop, apply, drop, push).
fn stash_subcommand(lower: &str) -> &str {
    lower
        .strip_prefix("git stash")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("")
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

pub struct GitOptimizer {
    log_max_entries: usize,
    log_default_limit: usize,
    log_line_max_chars: usize,
    diff_max_hunk_lines: usize,
    diff_max_total_lines: usize,
    branch_max_local: usize,
    branch_max_remote: usize,
}

impl Default for GitOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GitOptimizer {
    pub fn new() -> Self {
        Self::from_config(&GitOptimizerConfig::default())
    }

    /// Create a `GitOptimizer` from the configuration.
    pub fn from_config(cfg: &GitOptimizerConfig) -> Self {
        Self {
            log_max_entries: cfg.log_max_entries,
            log_default_limit: cfg.log_default_limit,
            log_line_max_chars: cfg.log_line_max_chars,
            diff_max_hunk_lines: cfg.diff_max_hunk_lines,
            diff_max_total_lines: cfg.diff_max_total_lines,
            branch_max_local: cfg.branch_max_local,
            branch_max_remote: cfg.branch_max_remote,
        }
    }
}

impl Optimizer for GitOptimizer {
    fn name(&self) -> &'static str {
        "git"
    }

    fn can_handle(&self, ctx: &CommandContext) -> bool {
        let lower = ctx.core.to_ascii_lowercase();
        let Some(sub) = classify(&lower) else {
            return false;
        };

        match sub {
            // Skip if user already requested a compact or verbose format.
            GitSubcommand::Status => {
                !has_flag(&lower, &["--short", "-s", "--porcelain", "-v", "--verbose"])
            }
            // Skip stat-only requests (already compact).
            GitSubcommand::Diff => !has_flag(&lower, &["--stat", "--numstat", "--shortstat"]),
            // Skip destructive / rename / copy branch operations.
            GitSubcommand::Branch => !has_flag(&lower, &["-d", "-D", "-m", "-M", "-c", "-C"]),
            // Skip if user specified a custom display format.
            GitSubcommand::Show => !has_flag(&lower, &["--stat", "--format", "--pretty"]),
            // Skip worktree action commands (add, remove, prune, etc.).
            GitSubcommand::Worktree => !has_flag(
                &lower,
                &["add", "remove", "prune", "lock", "unlock", "move"],
            ),
            _ => true,
        }
    }

    fn optimize_output(&self, ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput> {
        let lower = ctx.core.to_ascii_lowercase();
        let subcommand =
            classify(&lower).ok_or_else(|| anyhow!("git command not supported by optimizer"))?;

        let optimized = match subcommand {
            GitSubcommand::Status => optimize_status(ctx)?,
            GitSubcommand::Log => optimize_log(
                ctx,
                &lower,
                raw_output,
                self.log_max_entries,
                self.log_default_limit,
                self.log_line_max_chars,
            )?,
            GitSubcommand::Diff => compact_diff_with_stat(
                raw_output,
                self.diff_max_hunk_lines,
                self.diff_max_total_lines,
            ),
            GitSubcommand::Branch => {
                compact_git_branches(raw_output, self.branch_max_local, self.branch_max_remote)
            }
            GitSubcommand::Show => compact_git_show(
                raw_output,
                self.diff_max_hunk_lines,
                self.diff_max_total_lines,
            ),
            GitSubcommand::Stash => optimize_stash(
                &lower,
                raw_output,
                self.diff_max_hunk_lines,
                self.diff_max_total_lines,
            ),
            GitSubcommand::Worktree => compact_worktree_list(raw_output),
            GitSubcommand::ShortStatus => summarize_git_operation(&lower, raw_output),
        };

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Status — structured porcelain display
// ---------------------------------------------------------------------------

/// Run `git status --porcelain -b` and format into a structured display.
fn optimize_status(ctx: &CommandContext) -> Result<String> {
    let output =
        optimize_with_substitution(ctx.original, "git status", "git status --porcelain -b")?;
    Ok(format_porcelain_status(&output))
}

/// Parse porcelain output into a structured, token-efficient display.
///
/// Input format:
/// ```text
/// ## main...origin/main [ahead 1]
///  M src/main.rs
/// A  new_file.rs
/// ?? untracked.txt
/// ```
fn format_porcelain_status(porcelain: &str) -> String {
    let lines: Vec<&str> = porcelain.lines().collect();
    if lines.is_empty() {
        return "clean".to_string();
    }

    let mut output = String::new();

    // Parse branch info from ## line
    if let Some(branch_line) = lines.first()
        && let Some(branch) = branch_line.strip_prefix("## ")
    {
        output.push_str(&format!("branch: {branch}\n"));
    }

    let mut staged: Vec<&str> = Vec::new();
    let mut modified: Vec<&str> = Vec::new();
    let mut untracked: Vec<&str> = Vec::new();
    let mut conflicts = 0usize;

    for line in lines.iter().skip(1) {
        if line.len() < 3 {
            continue;
        }
        let status = line.as_bytes();
        let file = &line[3..];

        // First byte = index (staging area)
        match status[0] {
            b'M' | b'A' | b'D' | b'R' | b'C' => staged.push(file),
            b'U' => conflicts += 1,
            _ => {}
        }

        // Second byte = worktree (working directory)
        match status[1] {
            b'M' | b'D' => modified.push(file),
            _ => {}
        }

        if status[0] == b'?' && status[1] == b'?' {
            untracked.push(file);
        }
    }

    if staged.is_empty() && modified.is_empty() && untracked.is_empty() && conflicts == 0 {
        output.push_str("clean");
        return output.trim_end().to_string();
    }

    if !staged.is_empty() {
        output.push_str(&format!("staged ({}): ", staged.len()));
        append_file_list(&mut output, &staged, 5);
        output.push('\n');
    }
    if !modified.is_empty() {
        output.push_str(&format!("modified ({}): ", modified.len()));
        append_file_list(&mut output, &modified, 5);
        output.push('\n');
    }
    if !untracked.is_empty() {
        output.push_str(&format!("untracked ({}): ", untracked.len()));
        append_file_list(&mut output, &untracked, 3);
        output.push('\n');
    }
    if conflicts > 0 {
        output.push_str(&format!("conflicts: {conflicts}\n"));
    }

    output.trim_end().to_string()
}

fn append_file_list(output: &mut String, files: &[&str], max: usize) {
    let show = files.len().min(max);
    for (i, file) in files.iter().take(show).enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push_str(file);
    }
    if files.len() > show {
        output.push_str(&format!(", +{} more", files.len() - show));
    }
}

// ---------------------------------------------------------------------------
// Log — argument-aware substitution
// ---------------------------------------------------------------------------

/// Apply smart defaults for `git log`, respecting user-provided flags.
///
/// - Adds `--oneline` only if the user hasn't specified `--oneline`, `--pretty`,
///   or `--format`.
/// - Adds `-n 20` only if the user hasn't specified a numeric limit.
fn optimize_log(
    ctx: &CommandContext,
    core: &str,
    raw_text: &str,
    log_max_entries: usize,
    log_default_limit: usize,
    log_line_max_chars: usize,
) -> Result<String> {
    let has_format = has_flag(core, &["--oneline", "--pretty", "--format"]);
    let has_limit = has_numeric_limit(core) || has_flag(core, &["-n"]);

    if has_format && has_limit {
        // User already specified both — just filter the raw output for length.
        return Ok(filter_log_output(
            raw_text,
            log_max_entries,
            log_line_max_chars,
        ));
    }

    // Build substitution target with only the missing defaults.
    let mut to = String::from("git log");
    if !has_format {
        to.push_str(" --oneline");
    }
    if !has_limit {
        to.push_str(&format!(" -n {log_default_limit}"));
    }

    optimize_with_substitution(ctx.original, "git log", &to)
}

/// Truncate and cap long log output.
fn filter_log_output(output: &str, limit: usize, line_max_chars: usize) -> String {
    output
        .lines()
        .take(limit)
        .map(|line| {
            if line.len() > line_max_chars {
                let truncated: String = line.chars().take(line_max_chars - 3).collect();
                format!("{truncated}...")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Diff — stat summary + compact hunks
// ---------------------------------------------------------------------------

/// Produce a stat summary followed by compacted diff hunks.
fn compact_diff_with_stat(
    raw_output: &str,
    max_hunk_lines: usize,
    max_total_lines: usize,
) -> String {
    if raw_output.trim().is_empty() {
        return "No changes".to_string();
    }

    let stat = generate_diff_stat(raw_output);
    let hunks = compact_diff_hunks(raw_output, max_hunk_lines, max_total_lines);

    if stat.is_empty() && hunks.is_empty() {
        return raw_output.trim().to_string();
    }

    let mut result = String::new();
    if !stat.is_empty() {
        result.push_str(&stat);
    }
    if !hunks.is_empty() {
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.push_str(&hunks);
    }
    result
}

/// Compute a stat-like summary from raw diff output (no extra command needed).
fn generate_diff_stat(diff_text: &str) -> String {
    let mut files: Vec<(String, usize, usize)> = Vec::new();
    let mut current_file = String::new();
    let mut added = 0usize;
    let mut removed = 0usize;

    for line in diff_text.lines() {
        if line.starts_with("diff --git") {
            if !current_file.is_empty() {
                files.push((current_file.clone(), added, removed));
            }
            current_file = line.split(" b/").nth(1).unwrap_or("unknown").to_string();
            added = 0;
            removed = 0;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }
    if !current_file.is_empty() {
        files.push((current_file, added, removed));
    }

    if files.is_empty() {
        return String::new();
    }

    let mut result = Vec::new();
    for (file, a, r) in &files {
        result.push(format!(" {file} | +{a} -{r}"));
    }
    let total_a: usize = files.iter().map(|(_, a, _)| *a).sum();
    let total_r: usize = files.iter().map(|(_, _, r)| *r).sum();
    let n = files.len();
    result.push(format!(
        " {n} file{} changed, {total_a} insertion{}(+), {total_r} deletion{}(-)",
        if n == 1 { "" } else { "s" },
        if total_a == 1 { "" } else { "s" },
        if total_r == 1 { "" } else { "s" },
    ));
    result.join("\n")
}

/// Compact diff hunks with per-hunk line limits.
fn compact_diff_hunks(diff_text: &str, max_hunk_lines: usize, max_total_lines: usize) -> String {
    let mut kept = Vec::new();
    let mut hunk_lines = 0usize;

    for line in diff_text.lines() {
        if line.starts_with("diff --git") {
            hunk_lines = 0;
            kept.push(line);
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ")
        {
            kept.push(line);
        } else if line.starts_with("@@ ") {
            hunk_lines = 0;
            kept.push(line);
        } else if line.starts_with('+') || line.starts_with('-') {
            hunk_lines += 1;
            if hunk_lines <= max_hunk_lines {
                kept.push(line);
            } else if hunk_lines == max_hunk_lines + 1 {
                kept.push("  ...(hunk truncated)");
            }
        }

        if kept.len() >= max_total_lines {
            kept.push("...(diff truncated)");
            break;
        }
    }

    kept.join("\n")
}

// ---------------------------------------------------------------------------
// Branch — compact list with remote dedup
// ---------------------------------------------------------------------------

fn compact_git_branches(raw_output: &str, max_local: usize, max_remote: usize) -> String {
    let mut current = String::new();
    let mut local: Vec<String> = Vec::new();
    let mut remote: Vec<String> = Vec::new();

    for line in raw_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(branch) = trimmed.strip_prefix("* ") {
            current = branch.to_string();
        } else if let Some(remote_branch) = trimmed.strip_prefix("remotes/origin/") {
            if remote_branch.starts_with("HEAD ") {
                continue;
            }
            remote.push(remote_branch.to_string());
        } else {
            local.push(trimmed.to_string());
        }
    }

    let mut result = Vec::new();

    // Summary line: total counts at a glance.
    let total = 1_usize.max(local.len() + if current.is_empty() { 0 } else { 1 });
    let remote_count = remote.len();
    if remote_count > 0 {
        result.push(format!("branches: {total} local, {remote_count} remote"));
    } else {
        result.push(format!("branches: {total} local"));
    }

    if !current.is_empty() {
        result.push(format!("* {current}"));
    }
    for branch in local.iter().take(max_local) {
        result.push(format!("  {branch}"));
    }
    if local.len() > max_local {
        result.push(format!("  +{} more", local.len() - max_local));
    }

    if !remote.is_empty() {
        let remote_only: Vec<&String> = remote
            .iter()
            .filter(|r| *r != &current && !local.contains(r))
            .collect();
        if !remote_only.is_empty() {
            result.push(format!("  remote-only ({}):", remote_only.len()));
            for b in remote_only.iter().take(max_remote) {
                result.push(format!("    {b}"));
            }
            if remote_only.len() > max_remote {
                result.push(format!("    +{} more", remote_only.len() - max_remote));
            }
        }
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// Show — commit summary + stat + compact diff
// ---------------------------------------------------------------------------

fn compact_git_show(raw_output: &str, max_hunk_lines: usize, max_total_lines: usize) -> String {
    // Split at first "diff --git" to separate metadata from diff.
    let (metadata, diff_part) = match raw_output.find("diff --git") {
        Some(pos) => (&raw_output[..pos], &raw_output[pos..]),
        None => return raw_output.trim().to_string(),
    };

    let mut result = String::new();

    // Keep metadata but collapse consecutive blank lines.
    let mut prev_blank = false;
    for line in metadata.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        prev_blank = is_blank;
    }

    // Stat summary from the diff portion.
    let stat = generate_diff_stat(diff_part);
    if !stat.is_empty() {
        result.push_str(&stat);
        result.push('\n');
    }

    // Compact diff hunks.
    let compact = compact_diff_hunks(diff_part, max_hunk_lines, max_total_lines);
    if !compact.is_empty() {
        result.push('\n');
        result.push_str(&compact);
    }

    result.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Stash — list/show compact, actions summarized
// ---------------------------------------------------------------------------

fn optimize_stash(
    core: &str,
    raw_output: &str,
    max_hunk_lines: usize,
    max_total_lines: usize,
) -> String {
    let sub = stash_subcommand(core);
    match sub {
        "list" => compact_stash_list(raw_output),
        "show" => compact_diff_with_stat(raw_output, max_hunk_lines, max_total_lines),
        _ => summarize_stash_operation(sub, raw_output),
    }
}

fn compact_stash_list(raw_output: &str) -> String {
    if raw_output.trim().is_empty() {
        return "No stashes".to_string();
    }

    let mut result = Vec::new();
    for line in raw_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Format: "stash@{0}: WIP on main: abc1234 commit message"
        if let Some(colon_pos) = trimmed.find(": ") {
            let index = &trimmed[..colon_pos];
            let rest = &trimmed[colon_pos + 2..];
            // Strip "WIP on branch:" or "On branch:" prefix.
            let message = if let Some(second_colon) = rest.find(": ") {
                rest[second_colon + 2..].trim()
            } else {
                rest.trim()
            };
            result.push(format!("{index}: {message}"));
        } else {
            result.push(trimmed.to_string());
        }
    }
    result.join("\n")
}

fn summarize_stash_operation(sub: &str, raw_output: &str) -> String {
    let action = if sub.is_empty() { "push" } else { sub };
    let lower = raw_output.to_ascii_lowercase();

    if lower.contains("error") || lower.contains("fatal") {
        let first_line = raw_output
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(raw_output);
        format!("git stash {action}: failed - {}", first_line.trim())
    } else if lower.contains("no local changes") || lower.contains("no stash") {
        format!("git stash {action}: nothing to stash")
    } else {
        format!("git stash {action}: ok")
    }
}

// ---------------------------------------------------------------------------
// Worktree — compact list
// ---------------------------------------------------------------------------

fn compact_worktree_list(raw_output: &str) -> String {
    let lines: Vec<&str> = raw_output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return "No worktrees".to_string();
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// ShortStatus — push / pull / fetch / add / commit summaries
// ---------------------------------------------------------------------------

fn summarize_git_operation(command: &str, raw_output: &str) -> String {
    let action = command.split_whitespace().nth(1).unwrap_or("operation");

    let lower = raw_output.to_ascii_lowercase();
    let has_error =
        lower.contains("error") || lower.contains("fatal") || lower.contains("rejected");

    if has_error {
        let first_line = raw_output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(raw_output);
        format!("git {action}: failed - {}", first_line.trim())
    } else {
        format!("git {action}: ok")
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::CommandContext;

    // classify -----------------------------------------------------------

    #[test]
    fn classifies_all_subcommands() {
        assert_eq!(classify("git status"), Some(GitSubcommand::Status));
        assert_eq!(classify("git log --oneline"), Some(GitSubcommand::Log));
        assert_eq!(classify("git diff --cached"), Some(GitSubcommand::Diff));
        assert_eq!(classify("git branch -a"), Some(GitSubcommand::Branch));
        assert_eq!(classify("git show abc1234"), Some(GitSubcommand::Show));
        assert_eq!(classify("git stash list"), Some(GitSubcommand::Stash));
        assert_eq!(classify("git stash"), Some(GitSubcommand::Stash));
        assert_eq!(classify("git worktree list"), Some(GitSubcommand::Worktree));
        assert_eq!(
            classify("git push origin main"),
            Some(GitSubcommand::ShortStatus)
        );
        assert_eq!(classify("git pull"), Some(GitSubcommand::ShortStatus));
        assert_eq!(
            classify("git fetch --all"),
            Some(GitSubcommand::ShortStatus)
        );
        assert_eq!(classify("git add ."), Some(GitSubcommand::ShortStatus));
        assert_eq!(
            classify("git commit -m \"msg\""),
            Some(GitSubcommand::ShortStatus)
        );
        assert_eq!(classify("cargo test"), None);
        assert_eq!(classify("git rebase"), None);
    }

    // can_handle — basic routing ----------------------------------------

    #[test]
    fn detects_git_with_command_prefix() {
        assert!(GitOptimizer::new().can_handle(&CommandContext::new("cd /repo && git status")));
        assert!(GitOptimizer::new().can_handle(&CommandContext::new("git diff")));
        assert!(!GitOptimizer::new().can_handle(&CommandContext::new("npm test")));
    }

    #[test]
    fn handles_new_subcommands() {
        let opt = GitOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("git show abc1234")));
        assert!(opt.can_handle(&CommandContext::new("git stash list")));
        assert!(opt.can_handle(&CommandContext::new("git stash show")));
        assert!(opt.can_handle(&CommandContext::new("git worktree list")));
        assert!(opt.can_handle(&CommandContext::new("git worktree")));
    }

    // can_handle — action-mode passthrough ------------------------------

    #[test]
    fn branch_action_flags_passthrough() {
        let opt = GitOptimizer::new();
        assert!(!opt.can_handle(&CommandContext::new("git branch -d feature")));
        assert!(!opt.can_handle(&CommandContext::new("git branch -D feature")));
        assert!(!opt.can_handle(&CommandContext::new("git branch -m old new")));
        assert!(!opt.can_handle(&CommandContext::new("git branch -M old new")));
        assert!(!opt.can_handle(&CommandContext::new("git branch -c old new")));
        // List mode still handled
        assert!(opt.can_handle(&CommandContext::new("git branch")));
        assert!(opt.can_handle(&CommandContext::new("git branch -a")));
    }

    #[test]
    fn worktree_action_verbs_passthrough() {
        let opt = GitOptimizer::new();
        assert!(!opt.can_handle(&CommandContext::new("git worktree add ../feat feature")));
        assert!(!opt.can_handle(&CommandContext::new("git worktree remove ../feat")));
        assert!(!opt.can_handle(&CommandContext::new("git worktree prune")));
    }

    // can_handle — skip already-compact flags ---------------------------

    #[test]
    fn status_compact_flags_passthrough() {
        let opt = GitOptimizer::new();
        assert!(!opt.can_handle(&CommandContext::new("git status --short")));
        assert!(!opt.can_handle(&CommandContext::new("git status -s")));
        assert!(!opt.can_handle(&CommandContext::new("git status --porcelain")));
        assert!(!opt.can_handle(&CommandContext::new("git status -v")));
        // Bare status IS handled
        assert!(opt.can_handle(&CommandContext::new("git status")));
    }

    #[test]
    fn diff_stat_flags_passthrough() {
        let opt = GitOptimizer::new();
        assert!(!opt.can_handle(&CommandContext::new("git diff --stat")));
        assert!(!opt.can_handle(&CommandContext::new("git diff --numstat")));
        assert!(!opt.can_handle(&CommandContext::new("git diff --shortstat")));
        // Bare diff IS handled
        assert!(opt.can_handle(&CommandContext::new("git diff")));
        assert!(opt.can_handle(&CommandContext::new("git diff --cached")));
    }

    #[test]
    fn show_format_flags_passthrough() {
        let opt = GitOptimizer::new();
        assert!(!opt.can_handle(&CommandContext::new("git show --stat")));
        assert!(!opt.can_handle(&CommandContext::new("git show --pretty=format:%H")));
        assert!(!opt.can_handle(&CommandContext::new("git show --format=oneline")));
        // Bare show IS handled
        assert!(opt.can_handle(&CommandContext::new("git show")));
        assert!(opt.can_handle(&CommandContext::new("git show abc1234")));
    }

    // has_flag / has_numeric_limit helpers --------------------------------

    #[test]
    fn has_flag_detects_exact_and_prefix() {
        assert!(has_flag("git log --oneline", &["--oneline"]));
        assert!(has_flag("git show --pretty=format:%H", &["--pretty"]));
        assert!(has_flag("git status -s", &["-s"]));
        assert!(!has_flag("git status", &["-s", "--short"]));
        assert!(!has_flag("git diff", &["--stat"]));
    }

    #[test]
    fn has_numeric_limit_detects_limits() {
        assert!(has_numeric_limit("git log -10"));
        assert!(has_numeric_limit("git log -n 20"));
        assert!(has_numeric_limit("git log -5 --oneline"));
        assert!(!has_numeric_limit("git log --oneline"));
        assert!(!has_numeric_limit("git log -p")); // -p is not numeric
    }

    // format_porcelain_status -------------------------------------------

    #[test]
    fn porcelain_clean_tree() {
        let porcelain = "## main...origin/main\n";
        let result = format_porcelain_status(porcelain);
        assert!(result.contains("branch: main...origin/main"));
        assert!(result.contains("clean"));
    }

    #[test]
    fn porcelain_empty() {
        let result = format_porcelain_status("");
        assert_eq!(result, "clean");
    }

    #[test]
    fn porcelain_mixed_changes() {
        let porcelain = "## main\nM  staged.rs\n M modified.rs\nA  added.rs\n?? untracked.txt\n";
        let result = format_porcelain_status(porcelain);
        assert!(result.contains("branch: main"));
        assert!(result.contains("staged (2):"));
        assert!(result.contains("staged.rs"));
        assert!(result.contains("added.rs"));
        assert!(result.contains("modified (1):"));
        assert!(result.contains("modified.rs"));
        assert!(result.contains("untracked (1):"));
        assert!(result.contains("untracked.txt"));
    }

    #[test]
    fn porcelain_truncates_long_lists() {
        let mut porcelain = String::from("## main\n");
        for i in 0..8 {
            porcelain.push_str(&format!("?? file{i}.txt\n"));
        }
        let result = format_porcelain_status(&porcelain);
        assert!(result.contains("untracked (8):"));
        assert!(result.contains("+5 more"));
    }

    // generate_diff_stat ------------------------------------------------

    #[test]
    fn generates_stat_from_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index 111..222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
+    println!(\"world\");
-    old_line();
 }
";
        let stat = generate_diff_stat(diff);
        assert!(stat.contains("src/main.rs | +2 -1"));
        assert!(stat.contains("1 file changed, 2 insertions(+), 1 deletion(-)"));
    }

    #[test]
    fn stat_multi_file() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
+line
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
-line
-line
";
        let stat = generate_diff_stat(diff);
        assert!(stat.contains("a.rs | +1 -0"));
        assert!(stat.contains("b.rs | +0 -2"));
        assert!(stat.contains("2 files changed"));
    }

    // compact_diff_hunks ------------------------------------------------

    #[test]
    fn truncates_large_hunks() {
        let mut diff =
            String::from("diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,30 +1,30 @@\n");
        for i in 0..30 {
            diff.push_str(&format!("+line {i}\n"));
        }
        let compact = compact_diff_hunks(&diff, 15, 200);
        let plus_lines = compact
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .count();
        assert!(plus_lines <= 15);
        assert!(compact.contains("...(hunk truncated)"));
    }

    #[test]
    fn resets_hunk_counter_per_hunk() {
        let mut diff = String::from("diff --git a/f.rs b/f.rs\n@@ -1 +1 @@\n");
        for _ in 0..20 {
            diff.push_str("+line\n");
        }
        diff.push_str("@@ -30 +30 @@\n");
        for _ in 0..5 {
            diff.push_str("+line2\n");
        }
        let compact = compact_diff_hunks(&diff, 15, 200);
        // Second hunk should be fully included (only 5 lines)
        let hunk2_lines = compact.lines().filter(|l| l.starts_with("+line2")).count();
        assert_eq!(hunk2_lines, 5);
    }

    // compact_git_branches (improved) -----------------------------------

    #[test]
    fn compacts_local_branches() {
        let raw = "  main\n* feature/new-api\n  release\n";
        let compact = compact_git_branches(raw, 20, 10);
        assert!(compact.contains("branches: 3 local"));
        assert!(compact.contains("* feature/new-api"));
        assert!(compact.contains("main"));
        assert!(compact.contains("release"));
    }

    #[test]
    fn compacts_branches_with_remotes() {
        let raw = "\
* main
  feature/auth
  remotes/origin/HEAD -> origin/main
  remotes/origin/main
  remotes/origin/feature/auth
  remotes/origin/release/v2
";
        let compact = compact_git_branches(raw, 20, 10);
        assert!(compact.contains("branches: 2 local, 3 remote"));
        assert!(compact.contains("* main"));
        assert!(compact.contains("feature/auth"));
        assert!(compact.contains("remote-only (1):"));
        assert!(compact.contains("release/v2"));
        // main and feature/auth are local — should NOT appear in remote-only
        assert!(!compact.contains("    main"));
    }

    #[test]
    fn truncates_many_local_branches() {
        let mut raw = String::from("* current\n");
        for i in 0..40 {
            raw.push_str(&format!("  branch-{i:03}\n"));
        }
        let compact = compact_git_branches(&raw, 20, 10);
        assert!(compact.contains("branches: 41 local"));
        assert!(compact.contains("* current"));
        // Only first 20 non-current branches shown
        assert!(compact.contains("branch-019"));
        assert!(!compact.contains("branch-020"));
        assert!(compact.contains("+20 more"));
    }

    // compact_git_show --------------------------------------------------

    #[test]
    fn compacts_show_output() {
        let raw = "\
commit abc1234
Author: Test <test@example.com>
Date:   Thu Feb 13 2026

    Fix the thing

diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    old();
+    new();
 }
";
        let compact = compact_git_show(raw, 15, 200);
        assert!(compact.contains("commit abc1234"));
        assert!(compact.contains("Fix the thing"));
        assert!(compact.contains("src/main.rs | +1 -1"));
        assert!(compact.contains("+    new();"));
    }

    #[test]
    fn show_without_diff() {
        let raw = "commit abc1234\nAuthor: Test\n\n    Message\n";
        let compact = compact_git_show(raw, 15, 200);
        assert!(compact.contains("commit abc1234"));
        assert!(compact.contains("Message"));
    }

    // compact_stash_list ------------------------------------------------

    #[test]
    fn compacts_stash_list() {
        let raw = "stash@{0}: WIP on main: abc1234 fix login\nstash@{1}: On feature: def5678 wip\n";
        let compact = compact_stash_list(raw);
        assert!(compact.contains("stash@{0}: abc1234 fix login"));
        assert!(compact.contains("stash@{1}: def5678 wip"));
    }

    #[test]
    fn stash_list_empty() {
        assert_eq!(compact_stash_list(""), "No stashes");
    }

    // summarize_stash_operation -----------------------------------------

    #[test]
    fn stash_push_success() {
        let result = summarize_stash_operation("", "Saved working directory\n");
        assert_eq!(result, "git stash push: ok");
    }

    #[test]
    fn stash_pop_failure() {
        let result = summarize_stash_operation("pop", "error: conflict\n");
        assert!(result.contains("failed"));
    }

    #[test]
    fn stash_nothing_to_stash() {
        let result = summarize_stash_operation("", "No local changes to save\n");
        assert!(result.contains("nothing to stash"));
    }

    // compact_worktree_list ---------------------------------------------

    #[test]
    fn compacts_worktree_output() {
        let raw = "/home/user/project  abc1234 [main]\n/home/user/wt/feat  def5678 [feature]\n";
        let compact = compact_worktree_list(raw);
        assert!(compact.contains("[main]"));
        assert!(compact.contains("[feature]"));
    }

    #[test]
    fn worktree_empty() {
        assert_eq!(compact_worktree_list(""), "No worktrees");
    }

    // summarize_git_operation -------------------------------------------

    #[test]
    fn summarizes_push_result() {
        let summary = summarize_git_operation("git push", "Everything up-to-date\n");
        assert_eq!(summary, "git push: ok");

        let failed = summarize_git_operation("git pull", "error: could not fetch\n");
        assert!(failed.contains("failed"));
    }

    #[test]
    fn summarizes_rejected_push() {
        let result = summarize_git_operation(
            "git push",
            "To github.com:user/repo\n ! [rejected] main -> main (non-fast-forward)\n",
        );
        assert!(result.contains("failed"));
    }

    // filter_log_output -------------------------------------------------

    #[test]
    fn filter_log_truncates_long_lines() {
        let long = format!("abc1234 {}", "x".repeat(200));
        let result = filter_log_output(&long, 10, 120);
        assert!(result.len() < long.len());
        assert!(result.ends_with("..."));
    }

    #[test]
    fn filter_log_caps_line_count() {
        let lines: String = (0..30)
            .map(|i| format!("hash{i} message {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = filter_log_output(&lines, 5, 120);
        assert_eq!(result.lines().count(), 5);
    }

    // stash_subcommand helper -------------------------------------------

    #[test]
    fn extracts_stash_subcommand() {
        assert_eq!(stash_subcommand("git stash list"), "list");
        assert_eq!(stash_subcommand("git stash show -p"), "show");
        assert_eq!(stash_subcommand("git stash pop"), "pop");
        assert_eq!(stash_subcommand("git stash"), "");
    }
}
