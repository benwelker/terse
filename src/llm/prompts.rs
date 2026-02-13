//! Category-aware prompt templates for the LLM Smart Path.
//!
//! Each prompt instructs the LLM to condense command output while preserving
//! critical information for an AI coding assistant. Prompts include:
//!
//! - A role/context preamble
//! - Category-specific preservation rules
//! - A few-shot example (before → after)
//! - The actual raw output to condense
//!
//! The [`build_prompt`] function selects the best category based on the
//! command text and assembles the full prompt string.

// ---------------------------------------------------------------------------
// Command categories
// ---------------------------------------------------------------------------

/// Broad categories used to select a prompt template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    VersionControl,
    FileOperations,
    BuildTest,
    ContainerTools,
    Logs,
    Generic,
}

impl std::fmt::Display for CommandCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionControl => write!(f, "version_control"),
            Self::FileOperations => write!(f, "file_operations"),
            Self::BuildTest => write!(f, "build_test"),
            Self::ContainerTools => write!(f, "container_tools"),
            Self::Logs => write!(f, "logs"),
            Self::Generic => write!(f, "generic"),
        }
    }
}

/// Classify a command string into a [`CommandCategory`].
///
/// Uses prefix matching on the core command text (already extracted by the
/// matching engine). Falls back to [`CommandCategory::Generic`] if nothing
/// more specific matches.
pub fn classify_command(command: &str) -> CommandCategory {
    let lower = command.to_ascii_lowercase();

    if lower.starts_with("git ") || lower.starts_with("svn ") || lower.starts_with("hg ") {
        return CommandCategory::VersionControl;
    }

    // Check logs before file operations — `tail -f` is a log command, not a
    // generic file operation, and `journalctl`/`dmesg` are always logs.
    if lower.starts_with("journalctl")
        || lower.starts_with("dmesg")
        || lower.starts_with("tail -f")
    {
        return CommandCategory::Logs;
    }

    if lower.starts_with("ls")
        || lower.starts_with("dir")
        || lower.starts_with("find ")
        || lower.starts_with("cat ")
        || lower.starts_with("type ")
        || lower.starts_with("head ")
        || lower.starts_with("tail ")
        || lower.starts_with("wc ")
        || lower.starts_with("tree")
        || lower.starts_with("du ")
        || lower.starts_with("df ")
        || lower.starts_with("file ")
        || lower.starts_with("stat ")
    {
        return CommandCategory::FileOperations;
    }

    if lower.starts_with("cargo ")
        || lower.starts_with("npm ")
        || lower.starts_with("npx ")
        || lower.starts_with("yarn ")
        || lower.starts_with("pnpm ")
        || lower.starts_with("dotnet ")
        || lower.starts_with("make")
        || lower.starts_with("cmake ")
        || lower.starts_with("gradle ")
        || lower.starts_with("mvn ")
        || lower.starts_with("go ")
        || lower.starts_with("pytest")
        || lower.starts_with("python -m pytest")
        || lower.starts_with("msbuild")
    {
        return CommandCategory::BuildTest;
    }

    if lower.starts_with("docker ")
        || lower.starts_with("podman ")
        || lower.starts_with("kubectl ")
        || lower.starts_with("helm ")
    {
        return CommandCategory::ContainerTools;
    }

    // Broad heuristic: commands with "log" in the name that weren't caught by
    // more-specific prefixes above.
    if lower.contains("log") {
        return CommandCategory::Logs;
    }

    CommandCategory::Generic
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

/// Build the full prompt for the LLM, combining the category-specific
/// template with the raw command output.
pub fn build_prompt(command: &str, raw_output: &str) -> String {
    let category = classify_command(command);
    let template = template_for(category);

    format!(
        "{preamble}\n\n\
         ## Rules\n{rules}\n\n\
         ## Example\nBefore:\n```\n{example_before}\n```\n\
         After:\n```\n{example_after}\n```\n\n\
         ## Command\n`{command}`\n\n\
         ## Raw output\n```\n{raw_output}\n```\n\n\
         ## Condensed output\n",
        preamble = template.preamble,
        rules = template.rules,
        example_before = template.example_before,
        example_after = template.example_after,
        command = command,
        raw_output = truncate_for_prompt(raw_output, 6000),
    )
}

/// Truncate raw output to a maximum character length for the prompt.
///
/// Very large outputs would blow the context window. We cap at `max_chars`
/// and append a note so the LLM knows content was trimmed.
fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    let remaining = text.len() - max_chars;
    format!("{truncated}\n[... {remaining} more characters truncated]")
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

struct PromptTemplate {
    preamble: &'static str,
    rules: &'static str,
    example_before: &'static str,
    example_after: &'static str,
}

fn template_for(category: CommandCategory) -> PromptTemplate {
    match category {
        CommandCategory::VersionControl => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following version-control command output.",
            rules: "\
- Keep: branch name, changed files, conflict markers, ahead/behind status, commit hashes.\n\
- Remove: verbose status messages, decorative lines, repeated blank lines.\n\
- Preserve error messages exactly.\n\
- Output must be shorter than the input.",
            example_before: "\
On branch main\n\
Your branch is ahead of 'origin/main' by 2 commits.\n\
  (use \"git push\" to publish your local commits)\n\
\n\
Changes not staged for commit:\n\
  (use \"git add <file>...\" to update what will be committed)\n\
  (use \"git restore <file>...\" to discard changes in working directory)\n\
        modified:   src/main.rs",
            example_after: "\
branch: main (ahead 2)\nmodified: src/main.rs",
        },

        CommandCategory::FileOperations => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following file-operation command output.",
            rules: "\
- Keep: file/directory paths, sizes, important metadata.\n\
- Remove: permissions, owner, group, timestamps unless specifically relevant.\n\
- Group items logically when possible.\n\
- Output must be shorter than the input.",
            example_before: "\
total 48\n\
drwxr-xr-x  5 user staff  160 Jan 10 14:23 src\n\
-rw-r--r--  1 user staff  842 Jan 10 14:20 Cargo.toml\n\
-rw-r--r--  1 user staff 1205 Jan 10 14:23 README.md",
            example_after: "\
src/ (dir)\nCargo.toml (842B)\nREADME.md (1205B)",
        },

        CommandCategory::BuildTest => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following build/test command output.",
            rules: "\
- Keep: errors, warnings, test failures with file/line info, final summary.\n\
- Remove: passing-test output, progress indicators, download logs, compilation of individual crates.\n\
- Preserve the exact text of error/warning messages.\n\
- Output must be shorter than the input.",
            example_before: "\
   Compiling serde v1.0.195\n\
   Compiling serde_json v1.0.111\n\
   Compiling myapp v0.1.0\n\
error[E0308]: mismatched types\n\
 --> src/main.rs:42:5\n\
  |\n\
42 |     \"hello\"\n\
  |     ^^^^^^^ expected `i32`, found `&str`\n\
\n\
error: aborting due to 1 previous error",
            example_after: "\
error[E0308]: mismatched types\n  --> src/main.rs:42:5 — expected `i32`, found `&str`\n1 error",
        },

        CommandCategory::ContainerTools => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following container/orchestration command output.",
            rules: "\
- Keep: container names, images, status, ports, error messages.\n\
- Remove: full container IDs (truncate to 12 chars), verbose labels, creation timestamps.\n\
- Format as a compact table or list.\n\
- Output must be shorter than the input.",
            example_before: "\
CONTAINER ID   IMAGE          COMMAND       CREATED        STATUS        PORTS                    NAMES\n\
a1b2c3d4e5f6   nginx:latest   \"nginx -g…\"   2 hours ago    Up 2 hours    0.0.0.0:80->80/tcp       web\n\
f6e5d4c3b2a1   redis:7        \"redis-se…\"   3 hours ago    Up 3 hours    0.0.0.0:6379->6379/tcp   cache",
            example_after: "\
web    nginx:latest  Up 2h  :80->80\ncache  redis:7       Up 3h  :6379->6379",
        },

        CommandCategory::Logs => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following log output.",
            rules: "\
- Keep: errors, warnings, unique messages, first/last occurrence of repeated patterns.\n\
- Remove: debug-level noise, duplicate lines, heartbeat/health-check entries.\n\
- Summarize repeated patterns with counts (e.g., \"request handled (×42)\").\n\
- Output must be shorter than the input.",
            example_before: "\
2024-01-10 14:00:01 INFO  Server started on :8080\n\
2024-01-10 14:00:02 DEBUG Request handled: GET /health\n\
2024-01-10 14:00:03 DEBUG Request handled: GET /health\n\
2024-01-10 14:00:04 DEBUG Request handled: GET /health\n\
2024-01-10 14:00:05 ERROR Connection refused: database at localhost:5432\n\
2024-01-10 14:00:06 WARN  Retrying database connection (attempt 2)",
            example_after: "\
INFO  Server started on :8080\nDEBUG Request handled: GET /health (×3)\nERROR Connection refused: database at localhost:5432\nWARN  Retrying database connection (attempt 2)",
        },

        CommandCategory::Generic => PromptTemplate {
            preamble: "You are a concise output condenser for an AI coding assistant. \
                        Condense the following command output, preserving all critical information.",
            rules: "\
- Keep: errors, warnings, key data, file paths, status indicators.\n\
- Remove: decorative lines, repeated blank lines, verbose progress output.\n\
- Preserve the semantic meaning of the output.\n\
- Output must be shorter than the input.",
            example_before: "\
==============================================\n\
  Processing complete!\n\
==============================================\n\
\n\
Results:\n\
  Files processed: 42\n\
  Errors: 1\n\
  Error in file.txt: line 10 — invalid syntax\n\
\n\
Done.",
            example_after: "\
42 files processed, 1 error\n  file.txt:10 — invalid syntax",
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_git_commands() {
        assert_eq!(classify_command("git status"), CommandCategory::VersionControl);
        assert_eq!(classify_command("git log --oneline"), CommandCategory::VersionControl);
    }

    #[test]
    fn classify_file_commands() {
        assert_eq!(classify_command("ls -la"), CommandCategory::FileOperations);
        assert_eq!(classify_command("find . -name '*.rs'"), CommandCategory::FileOperations);
        assert_eq!(classify_command("cat README.md"), CommandCategory::FileOperations);
    }

    #[test]
    fn classify_build_commands() {
        assert_eq!(classify_command("cargo test"), CommandCategory::BuildTest);
        assert_eq!(classify_command("npm test"), CommandCategory::BuildTest);
        assert_eq!(classify_command("dotnet build"), CommandCategory::BuildTest);
    }

    #[test]
    fn classify_container_commands() {
        assert_eq!(classify_command("docker ps"), CommandCategory::ContainerTools);
        assert_eq!(classify_command("kubectl get pods"), CommandCategory::ContainerTools);
    }

    #[test]
    fn classify_log_commands() {
        assert_eq!(classify_command("journalctl -u myservice"), CommandCategory::Logs);
        assert_eq!(classify_command("tail -f /var/log/syslog"), CommandCategory::Logs);
    }

    #[test]
    fn classify_generic_commands() {
        assert_eq!(classify_command("whoami"), CommandCategory::Generic);
        assert_eq!(classify_command("curl http://example.com"), CommandCategory::Generic);
    }

    #[test]
    fn build_prompt_includes_output() {
        let prompt = build_prompt("git status", "On branch main\nnothing to commit");
        assert!(prompt.contains("git status"));
        assert!(prompt.contains("On branch main"));
        assert!(prompt.contains("Condensed output"));
    }

    #[test]
    fn truncate_short_text_unchanged() {
        let text = "short";
        assert_eq!(truncate_for_prompt(text, 100), "short");
    }

    #[test]
    fn truncate_long_text() {
        let text = "a".repeat(200);
        let result = truncate_for_prompt(&text, 100);
        assert!(result.contains("[... 100 more characters truncated]"));
        assert!(result.len() < 200);
    }
}
