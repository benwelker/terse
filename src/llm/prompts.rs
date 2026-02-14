//! Category-aware prompt templates for the LLM Smart Path.
//!
//! Each prompt instructs the LLM to condense command output while preserving
//! critical information for an AI coding assistant. Templates are split into:
//!
//! - A **system message** (role, rules, few-shot example)
//! - A **user message** (the raw output to condense)
//!
//! This two-message design maps directly to the Ollama `/api/chat` endpoint,
//! which applies the correct chat template tokens for each model
//! (Llama `<|start_header_id|>`, Qwen `<|im_start|>`, Gemma `<start_of_turn>`,
//! Phi `<|system|>`, etc.) automatically.
//!
//! The [`build_chat_messages`] function selects the best category based on
//! the command text and returns a `(system, user)` tuple.

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
    if lower.starts_with("journalctl") || lower.starts_with("dmesg") || lower.starts_with("tail -f")
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

/// Build chat messages for the Ollama `/api/chat` endpoint.
///
/// Returns `(system_message, user_message)`:
/// - **system**: role definition, category-specific rules, and a single
///   few-shot example. This stays constant for a given command category.
/// - **user**: the raw command output to condense plus a terse
///   instruction. Keeping the data in the *user* role leverages chat
///   template role boundaries, preventing small models from confusing the
///   example with the actual input.
///
/// Ollama applies the correct model-specific chat template tokens
/// (Llama `<|start_header_id|>`, Qwen `<|im_start|>`, etc.) so we
/// never need to hard-code special tokens ourselves.
pub fn build_chat_messages(command: &str, raw_output: &str) -> (String, String) {
    let category = classify_command(command);
    let template = template_for(category);
    let truncated = truncate_for_prompt(raw_output, 16_000);

    // System message: role + rules. No few-shot example — small models
    // (0.5B–3B) consistently parrot demonstrations instead of processing
    // the actual input. Pure instructions work better.
    let system = format!(
        "{preamble}\n\n\
         {rules}\n\n\
         Output ONLY the condensed version of the user's text. \
         No commands, no explanations, no commentary, no preamble.",
        preamble = template.preamble,
        rules = template.rules,
    );

    // User message: just the raw data with a minimal instruction.
    let user = format!(
        "Condense this `{command}` output:\n\n{raw_output}",
        command = command,
        raw_output = truncated,
    );

    (system, user)
}

/// Build a single combined prompt string (legacy, for `/api/generate`).
///
/// Prefer [`build_chat_messages`] with the `/api/chat` endpoint. This
/// function is kept for testing and fallback scenarios.
#[allow(dead_code)]
pub fn build_prompt(command: &str, raw_output: &str) -> String {
    let (system, user) = build_chat_messages(command, raw_output);
    format!("{system}\n\n{user}\n\nCONDENSED:\n")
}

/// Truncate raw output to a maximum character length for the prompt.
///
/// Very large outputs would blow the context window. We cap at `max_chars`
/// to leave room for the prompt template (~500 tokens) and the response
/// (~2048 tokens) within the model's context window.
///
/// Default budget: 16,000 chars ≈ 4,000 tokens, fitting comfortably in
/// the 8K-token context window alongside template (~500 tokens) and
/// response budget (up to 4,096 tokens).
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
}

/// Return the few-shot example "after" text for a given command.
///
/// Returns an empty string — few-shot examples have been removed because
/// small models (0.5B–3B) parrot them instead of condensing the actual
/// input. Kept as a function so [`crate::llm::validation`] compiles
/// without changes; the echo-detection check becomes a harmless no-op.
pub fn example_after_for(_command: &str) -> &'static str {
    ""
}

fn template_for(category: CommandCategory) -> PromptTemplate {
    match category {
        CommandCategory::VersionControl => PromptTemplate {
            preamble: "You are a concise output condenser. Shorten the text below. \
                        Do NOT generate commands, flags, or suggestions. Output ONLY a shorter version of the input text.",
            rules: "\
Rules:\n\
- For each commit in the input, output exactly one line: SHORT_HASH MESSAGE\n\
- Keep ALL commits from the input (do not skip any)\n\
- Use the first 7 characters of each commit hash\n\
- Keep only the first line of each commit message\n\
- Remove: author names, dates, merge info, blank lines, decorative lines, hint lines\n\
- Preserve branch names, ahead/behind status, error messages\n\
- Do NOT output git commands, flags, or format strings",
        },

        CommandCategory::FileOperations => PromptTemplate {
            preamble: "You are a concise output condenser. Your ONLY job is to shorten the text below. \
                        Do NOT generate commands. Do NOT explain anything. Just output a shorter version of the input text.",
            rules: "\
Keep: file/directory paths, sizes, important metadata.\n\
Remove: permissions, owner, group, timestamps unless specifically relevant.\n\
Group items logically when possible.\n\
Do NOT generate shell commands or flags. Do NOT write explanations.",
        },

        CommandCategory::BuildTest => PromptTemplate {
            preamble: "You are a concise output condenser. Your ONLY job is to shorten the text below. \
                        Do NOT generate commands. Do NOT explain anything. Just output a shorter version of the input text.",
            rules: "\
Keep: errors, warnings, test failures with file/line info, final summary.\n\
Remove: passing-test output, progress indicators, download logs, compilation of individual crates.\n\
Preserve the exact text of error/warning messages.\n\
Do NOT generate shell commands or flags. Do NOT write explanations.",
        },

        CommandCategory::ContainerTools => PromptTemplate {
            preamble: "You are a concise output condenser. Your ONLY job is to shorten the text below. \
                        Do NOT generate commands. Do NOT explain anything. Just output a shorter version of the input text.",
            rules: "\
Keep: container names, images, status, ports, error messages.\n\
Remove: full container IDs (truncate to 12 chars), verbose labels, creation timestamps.\n\
Format as a compact table or list.\n\
Do NOT generate shell commands or flags. Do NOT write explanations.",
        },

        CommandCategory::Logs => PromptTemplate {
            preamble: "You are a concise output condenser. Your ONLY job is to shorten the text below. \
                        Do NOT generate commands. Do NOT explain anything. Just output a shorter version of the input text.",
            rules: "\
Keep: errors, warnings, unique messages, first/last occurrence of repeated patterns.\n\
Remove: debug-level noise, duplicate lines, heartbeat/health-check entries.\n\
Summarize repeated patterns with counts (e.g., \"request handled (×42)\").\n\
Do NOT generate shell commands or flags. Do NOT write explanations.",
        },

        CommandCategory::Generic => PromptTemplate {
            preamble: "You are a concise output condenser. Your ONLY job is to shorten the text below. \
                        Do NOT generate commands. Do NOT explain anything. Just output a shorter version of the input text.",
            rules: "\
Keep: errors, warnings, key data, file paths, status indicators.\n\
Remove: decorative lines, repeated blank lines, verbose progress output.\n\
Preserve the semantic meaning of the output.\n\
Do NOT generate shell commands or flags. Do NOT write explanations.",
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
        assert_eq!(
            classify_command("git status"),
            CommandCategory::VersionControl
        );
        assert_eq!(
            classify_command("git log --oneline"),
            CommandCategory::VersionControl
        );
    }

    #[test]
    fn classify_file_commands() {
        assert_eq!(classify_command("ls -la"), CommandCategory::FileOperations);
        assert_eq!(
            classify_command("find . -name '*.rs'"),
            CommandCategory::FileOperations
        );
        assert_eq!(
            classify_command("cat README.md"),
            CommandCategory::FileOperations
        );
    }

    #[test]
    fn classify_build_commands() {
        assert_eq!(classify_command("cargo test"), CommandCategory::BuildTest);
        assert_eq!(classify_command("npm test"), CommandCategory::BuildTest);
        assert_eq!(classify_command("dotnet build"), CommandCategory::BuildTest);
    }

    #[test]
    fn classify_container_commands() {
        assert_eq!(
            classify_command("docker ps"),
            CommandCategory::ContainerTools
        );
        assert_eq!(
            classify_command("kubectl get pods"),
            CommandCategory::ContainerTools
        );
    }

    #[test]
    fn classify_log_commands() {
        assert_eq!(
            classify_command("journalctl -u myservice"),
            CommandCategory::Logs
        );
        assert_eq!(
            classify_command("tail -f /var/log/syslog"),
            CommandCategory::Logs
        );
    }

    #[test]
    fn classify_generic_commands() {
        assert_eq!(classify_command("whoami"), CommandCategory::Generic);
        assert_eq!(
            classify_command("curl http://example.com"),
            CommandCategory::Generic
        );
    }

    #[test]
    fn build_chat_messages_splits_roles() {
        let (system, user) = build_chat_messages("git status", "On branch main\nnothing to commit");
        // System contains rules, not the actual data
        assert!(system.contains("condense"));
        assert!(system.contains("No commands, no explanations"));
        // System should NOT contain any example output (removed to prevent parroting)
        assert!(!system.contains("branch: main (ahead 2)"));
        // User contains the actual data
        assert!(user.contains("git status"));
        assert!(user.contains("On branch main"));
    }

    #[test]
    fn build_prompt_legacy_includes_output() {
        let prompt = build_prompt("git status", "On branch main\nnothing to commit");
        assert!(prompt.contains("git status"));
        assert!(prompt.contains("On branch main"));
        assert!(prompt.contains("CONDENSED:"));
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
