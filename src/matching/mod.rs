//! Command matching engine for extracting and normalizing commands.
//!
//! Claude Code often wraps commands with `cd`, `&&` chains, environment variable
//! prefixes, subshell wrappers, or `sh -c` invocations. This module extracts the
//! core command for **matching purposes only** while the full original command is
//! always preserved for execution.
//!
//! # Design
//!
//! The matching engine is used in two places:
//! 1. **Hook** (`terse hook`): determines if any optimizer can handle the command
//!    and whether the command is already a terse invocation (loop guard).
//! 2. **Optimizers**: `can_handle()` and `optimize_output()` use the extracted
//!    core command for decision-making while passing the full original command to
//!    `run_shell_command()` so that `cd`, env vars, pipes, etc. execute correctly.
//!
//! # Example
//!
//! ```
//! use terse::matching::extract_core_command;
//!
//! // Claude sends: cd /repo && LANG=C git status
//! // Matching engine extracts: git status
//! // But terse rewrites: terse.exe run "cd /repo && LANG=C git status"
//! // So the full command executes in the correct directory with the env var.
//! assert_eq!(extract_core_command("cd /repo && LANG=C git status"), "git status");
//! ```

/// Extract the core executable command from a shell invocation.
///
/// Strips common wrappers to find the actual command for optimizer matching:
/// - Subshell: `(cmd)` → `cmd`
/// - Shell wrapper: `bash -c "cmd"` / `sh -c "cmd"` → `cmd`
/// - Chain prefix: `cd /repo && cmd` → `cmd` (last `&&`/`;` segment)
/// - Pipeline: `cmd | filter` → `cmd` (first pipe segment)
/// - Env vars: `LANG=C cmd` → `cmd`
///
/// Returns a trimmed `&str` slice into the original string, suitable for
/// prefix-matching against known command names.
pub fn extract_core_command(raw: &str) -> &str {
    let mut s = raw.trim();
    if s.is_empty() {
        return s;
    }

    // 1. Unwrap subshell wrappers: (cmd) → cmd
    s = unwrap_subshell(s);

    // 2. Unwrap shell wrappers: bash -c "cmd" / sh -c "cmd" → cmd
    s = unwrap_shell_wrapper(s);

    // After unwrapping, the inner command might itself be subshelled
    s = unwrap_subshell(s);

    // 3. Take the last segment after && or ; (handles cd /path && cmd)
    s = last_chain_segment(s);

    // 4. Take first command before pipe: cmd | filter → cmd
    s = first_pipe_segment(s);

    // 5. Strip environment variable assignments: LANG=C cmd → cmd
    s = strip_env_assignments(s);

    s
}

/// Check whether a command contains a heredoc (`<<`), which should always
/// passthrough without optimization.
///
/// Heredocs embed multi-line content inline and are structurally complex —
/// rewriting them risks breaking the document boundary. RTK-AI skips these
/// for the same reason.
pub fn contains_heredoc(command: &str) -> bool {
    let bytes = command.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i + 1 < len {
        // Skip characters inside single or double quotes
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            i += 1;
            while i < len && bytes[i] != quote {
                i += 1;
            }
            i += 1; // skip closing quote
            continue;
        }

        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            // Distinguish heredoc (<<) from process substitution (<<<)
            // <<< is a "here string", not a heredoc — still safe to skip
            return true;
        }

        i += 1;
    }

    false
}

/// Check whether a command is already a terse invocation (infinite loop guard).
///
/// Detects commands like `terse run "..."`, `terse.exe run "..."`, or
/// `"/path/to/terse.exe" run "..."`. Uses structural matching rather than a
/// naive substring search to avoid false positives with directory names that
/// happen to contain "terse" and "run".
pub fn is_terse_invocation(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();

    // Find the last occurrence of "terse" — in a path like
    // "C:\source\terse\target\terse.exe", the last one is the binary name.
    let Some(terse_pos) = lower.rfind("terse") else {
        return false;
    };

    let after_terse = &lower[terse_pos + 5..]; // skip "terse"

    // Skip optional ".exe" suffix and optional closing quote
    let after_suffix = after_terse.strip_prefix(".exe").unwrap_or(after_terse);
    let after_quote = after_suffix.strip_prefix('"').unwrap_or(after_suffix);

    // Must be followed by whitespace then "run"
    let after_ws = after_quote.trim_start();
    after_ws.starts_with("run")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Unwrap a single layer of subshell parentheses: `(cmd)` → `cmd`.
fn unwrap_subshell(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    }
}

/// Unwrap `bash -c "cmd"` / `sh -c "cmd"` shell wrappers.
fn unwrap_shell_wrapper(s: &str) -> &str {
    let trimmed = s.trim();
    for prefix in &["bash -c ", "sh -c "] {
        if let Some(rest) = ascii_prefix_strip(trimmed, prefix) {
            return strip_outer_quotes(rest.trim());
        }
    }
    trimmed
}

/// Case-insensitive prefix strip that returns the remainder as a slice.
fn ascii_prefix_strip<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    if s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Strip a single layer of matching outer quotes from a string.
fn strip_outer_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        return &s[1..s.len() - 1];
    }
    s
}

/// Take the last segment after `&&` or `;`.
///
/// Handles the common Claude pattern of `cd /path && actual_command`.
fn last_chain_segment(s: &str) -> &str {
    if let Some(pos) = s.rfind("&&") {
        return s[pos + 2..].trim();
    }
    if let Some(pos) = s.rfind(';') {
        return s[pos + 1..].trim();
    }
    s
}

/// Take the first command before a pipe (`|`), skipping logical-OR (`||`).
fn first_pipe_segment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let len = bytes.len();

    for i in 0..len {
        if bytes[i] != b'|' {
            continue;
        }
        // Skip logical-OR: ||
        if i + 1 < len && bytes[i + 1] == b'|' {
            continue;
        }
        // Skip second char of ||
        if i > 0 && bytes[i - 1] == b'|' {
            continue;
        }
        return s[..i].trim();
    }
    s
}

/// Strip leading environment variable assignments (`WORD=value` prefixes).
///
/// Iteratively removes `KEY=VALUE ` prefixes where KEY is a valid shell
/// identifier (`[A-Za-z_][A-Za-z0-9_]*`) and VALUE is an unquoted or
/// quoted token. Stops when the next token doesn't look like an assignment.
fn strip_env_assignments(s: &str) -> &str {
    let mut rest = s;

    loop {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            return trimmed;
        }

        // An env assignment must start with a letter or underscore
        let first = trimmed.as_bytes()[0];
        if !first.is_ascii_alphabetic() && first != b'_' {
            return trimmed;
        }

        // Find the = sign
        let Some(eq_pos) = trimmed.find('=') else {
            return trimmed;
        };

        // Everything before = must be a valid shell identifier
        let key = &trimmed[..eq_pos];
        if !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return trimmed;
        }

        // Skip past the value (possibly quoted)
        let after_eq = &trimmed[eq_pos + 1..];
        let value_end = skip_value(after_eq);

        if value_end >= after_eq.len() {
            // Value consumes the rest of the string — no command follows
            return trimmed;
        }

        rest = &after_eq[value_end..];
    }
}

/// Find the byte length of an env-var value (possibly quoted).
///
/// Returns the offset just past the value, so `&s[skip_value(s)..]` starts
/// at the whitespace (or end) after the value.
fn skip_value(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }

    let bytes = s.as_bytes();

    // Quoted value: find matching close quote
    if bytes[0] == b'"' || bytes[0] == b'\'' {
        let quote = bytes[0];
        for (i, &b) in bytes[1..].iter().enumerate() {
            if b == quote {
                return i + 2; // past the closing quote
            }
        }
        return s.len(); // unclosed quote — consume everything
    }

    // Unquoted: value extends to next whitespace
    s.find(char::is_whitespace).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // extract_core_command
    // -----------------------------------------------------------------------

    #[test]
    fn simple_command() {
        assert_eq!(extract_core_command("git status"), "git status");
    }

    #[test]
    fn cd_prefix_with_ampersand() {
        assert_eq!(extract_core_command("cd /repo && git status"), "git status");
    }

    #[test]
    fn cd_prefix_with_long_path() {
        assert_eq!(
            extract_core_command("cd /home/user/project && git log --oneline -n 5"),
            "git log --oneline -n 5"
        );
    }

    #[test]
    fn cd_prefix_with_semicolon() {
        assert_eq!(extract_core_command("cd /repo; git status"), "git status");
    }

    #[test]
    fn env_var_prefix() {
        assert_eq!(extract_core_command("LANG=C git diff"), "git diff");
    }

    #[test]
    fn multiple_env_vars() {
        assert_eq!(
            extract_core_command("LANG=C GIT_PAGER=cat git log"),
            "git log"
        );
    }

    #[test]
    fn git_dir_env() {
        assert_eq!(
            extract_core_command("GIT_DIR=/foo git status"),
            "git status"
        );
    }

    #[test]
    fn quoted_env_value() {
        assert_eq!(
            extract_core_command("FOO=\"bar baz\" git status"),
            "git status"
        );
    }

    #[test]
    fn single_quoted_env_value() {
        assert_eq!(
            extract_core_command("FOO='bar baz' git status"),
            "git status"
        );
    }

    #[test]
    fn subshell_wrapper() {
        assert_eq!(extract_core_command("(cd /repo && git diff)"), "git diff");
    }

    #[test]
    fn bash_wrapper_double_quotes() {
        assert_eq!(extract_core_command("bash -c \"git status\""), "git status");
    }

    #[test]
    fn sh_wrapper_single_quotes() {
        assert_eq!(extract_core_command("sh -c 'git status'"), "git status");
    }

    #[test]
    fn bash_wrapper_with_chain_inside() {
        assert_eq!(
            extract_core_command("bash -c \"cd /repo && git status\""),
            "git status"
        );
    }

    #[test]
    fn pipeline() {
        assert_eq!(extract_core_command("git log | head -20"), "git log");
    }

    #[test]
    fn pipeline_does_not_split_on_logical_or() {
        // || is a chain operator, not a pipe — the full string stays intact
        // after chain/pipe processing, starts_with("git") is still correct
        let core = extract_core_command("git status || echo failed");
        assert!(core.starts_with("git status"));
    }

    #[test]
    fn complex_cd_env_combo() {
        assert_eq!(
            extract_core_command("cd /home/user && LANG=C git status"),
            "git status"
        );
    }

    #[test]
    fn echo_not_matched_as_git() {
        let core = extract_core_command("echo 'git status'");
        assert!(
            core.starts_with("echo"),
            "expected echo prefix, got: {core}"
        );
    }

    #[test]
    fn grep_not_matched_as_git() {
        let core = extract_core_command("grep \"git status\" file.txt");
        assert!(
            core.starts_with("grep"),
            "expected grep prefix, got: {core}"
        );
    }

    #[test]
    fn command_with_equals_in_args_not_stripped() {
        // git log --format=oneline should NOT be treated as an env var assignment
        assert_eq!(
            extract_core_command("git log --format=oneline"),
            "git log --format=oneline"
        );
    }

    #[test]
    fn command_equals_in_args_after_cd() {
        assert_eq!(
            extract_core_command("cd /repo && git log --format=oneline"),
            "git log --format=oneline"
        );
    }

    #[test]
    fn preserves_command_arguments() {
        assert_eq!(
            extract_core_command("cd /repo && git diff --cached --stat"),
            "git diff --cached --stat"
        );
    }

    #[test]
    fn empty_command() {
        assert_eq!(extract_core_command(""), "");
        assert_eq!(extract_core_command("  "), "");
    }

    #[test]
    fn whitespace_only_after_cd() {
        assert_eq!(extract_core_command("cd /repo && "), "");
    }

    #[test]
    fn multiple_ampersand_chains() {
        // cd /repo && echo setup && git status → last segment is git status
        assert_eq!(
            extract_core_command("cd /repo && echo setup && git status"),
            "git status"
        );
    }

    #[test]
    fn subshell_with_env_and_cd() {
        assert_eq!(
            extract_core_command("(cd /repo && PAGER=cat git log)"),
            "git log"
        );
    }

    #[test]
    fn pipeline_with_cd_prefix() {
        assert_eq!(
            extract_core_command("cd /repo && cat file.txt | grep pattern"),
            "cat file.txt"
        );
    }

    #[test]
    fn non_git_command_extracted() {
        assert_eq!(extract_core_command("cd /app && npm test"), "npm test");
    }

    #[test]
    fn cargo_command_through_cd() {
        assert_eq!(
            extract_core_command("cd /project && cargo build --release"),
            "cargo build --release"
        );
    }

    // -----------------------------------------------------------------------
    // is_terse_invocation
    // -----------------------------------------------------------------------

    #[test]
    fn detects_simple_terse_run() {
        assert!(is_terse_invocation("terse run \"git status\""));
    }

    #[test]
    fn detects_terse_exe_run() {
        assert!(is_terse_invocation("terse.exe run \"git status\""));
    }

    #[test]
    fn detects_quoted_path_terse_run() {
        assert!(is_terse_invocation(
            "\"C:\\source\\terse\\target\\terse.exe\" run \"git status\""
        ));
    }

    #[test]
    fn detects_unix_path_terse_run() {
        assert!(is_terse_invocation(
            "/home/user/.terse/bin/terse run \"git status\""
        ));
    }

    #[test]
    fn rejects_plain_git_status() {
        assert!(!is_terse_invocation("git status"));
    }

    #[test]
    fn rejects_directory_named_terse_run() {
        // A repo path containing "terse" and a directory named "run" should NOT match
        assert!(!is_terse_invocation(
            "cd /my-terse-run-project && git status"
        ));
    }

    #[test]
    fn rejects_terse_in_path_segment() {
        assert!(!is_terse_invocation("cd /terse/run/project && git status"));
    }

    #[test]
    fn rejects_terse_without_run() {
        assert!(!is_terse_invocation("terse stats"));
    }

    // -----------------------------------------------------------------------
    // contains_heredoc
    // -----------------------------------------------------------------------

    #[test]
    fn detects_heredoc() {
        assert!(contains_heredoc("cat <<EOF\nhello\nEOF"));
    }

    #[test]
    fn detects_heredoc_with_dash() {
        assert!(contains_heredoc("cat <<-EOF\n\thello\nEOF"));
    }

    #[test]
    fn detects_here_string() {
        // <<< is a here-string — still complex enough to skip
        assert!(contains_heredoc("grep pattern <<<\"some text\""));
    }

    #[test]
    fn no_heredoc_in_simple_command() {
        assert!(!contains_heredoc("git status"));
    }

    #[test]
    fn no_heredoc_in_redirect() {
        // A single < is input redirect, not heredoc
        assert!(!contains_heredoc("sort < file.txt"));
    }

    #[test]
    fn no_false_positive_in_quoted_heredoc() {
        // << inside quotes is literal text, not a heredoc operator
        assert!(!contains_heredoc("echo \"use << for heredocs\""));
    }

    #[test]
    fn heredoc_after_pipe() {
        assert!(contains_heredoc("mysql -u root <<EOF\nSELECT 1;\nEOF"));
    }

    // -----------------------------------------------------------------------
    // Internal helper tests
    // -----------------------------------------------------------------------

    #[test]
    fn strip_env_does_not_eat_flags_with_equals() {
        // --format=oneline starts with -, not a letter → immediately returns
        assert_eq!(
            strip_env_assignments("--format=oneline"),
            "--format=oneline"
        );
    }

    #[test]
    fn strip_env_handles_path_value() {
        assert_eq!(
            strip_env_assignments("HOME=/tmp git status").trim(),
            "git status"
        );
    }

    #[test]
    fn first_pipe_ignores_double_pipe() {
        assert_eq!(
            first_pipe_segment("git status || echo fallback"),
            "git status || echo fallback"
        );
    }

    #[test]
    fn first_pipe_splits_single_pipe() {
        assert_eq!(first_pipe_segment("git log | head"), "git log");
    }
}
