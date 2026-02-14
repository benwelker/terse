/// Validation for LLM-generated output.
///
/// Before accepting an LLM response, we run a series of sanity checks to
/// ensure the output is usable. If any check fails the caller should fall
/// back to the raw command output.
///
/// Checks:
/// 1. **Non-empty** — LLM must return something.
/// 2. **Shorter than original** — condensation, not expansion.
/// 3. **No hallucination markers** — fabricated paths, invented status codes, etc.
use anyhow::{Result, anyhow};

/// Result of a validation pass.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ValidationResult {
    /// Whether the output is acceptable.
    pub valid: bool,
    /// Human-readable reason if validation failed.
    pub reason: Option<String>,
}

/// Validate an LLM response against the raw command output.
///
/// Returns `Ok(())` if the response passes all checks, or `Err` with a
/// description of the first failing check.
///
/// **Note:** call [`strip_preamble`] on the LLM output *before* passing it
/// here so that harmless conversational prefixes don't trigger the
/// hallucination/refusal check.
pub fn validate_llm_output(command: &str, raw_output: &str, llm_output: &str) -> Result<()> {
    check_non_empty(llm_output)?;
    check_shorter(raw_output, llm_output)?;
    check_no_hallucination_markers(llm_output)?;
    check_no_example_echo(command, llm_output)?;
    Ok(())
}

/// The LLM response must contain at least one non-whitespace character.
fn check_non_empty(llm_output: &str) -> Result<()> {
    if llm_output.trim().is_empty() {
        return Err(anyhow!("LLM returned empty output"));
    }
    Ok(())
}

/// The LLM response must be shorter than the raw output.
///
/// We allow a small margin (10%) to account for cases where the LLM adds
/// formatting characters while removing content.
fn check_shorter(raw_output: &str, llm_output: &str) -> Result<()> {
    let raw_len = raw_output.len();
    let llm_len = llm_output.trim().len();

    // Allow up to 110% of the original length as a safety margin
    let threshold = raw_len + raw_len / 10;

    if llm_len > threshold {
        return Err(anyhow!(
            "LLM output ({llm_len} chars) is longer than raw output ({raw_len} chars)"
        ));
    }
    Ok(())
}

/// Check for genuine hallucination / refusal markers in LLM output.
///
/// These patterns indicate the LLM refused to help or fabricated content
/// rather than condensing the input. Harmless conversational preambles
/// ("Here is the condensed …") are handled separately by [`strip_preamble`]
/// and are **not** checked here.
fn check_no_hallucination_markers(llm_output: &str) -> Result<()> {
    // --- Refusal patterns ---
    let refusal_markers = [
        "I apologize",
        "I'm sorry",
        "As an AI",
        "I cannot",
        "I can't fulfill",
        "I can't help",
        "I don't have access",
    ];

    let lower = llm_output.to_ascii_lowercase();
    for marker in &refusal_markers {
        if lower.contains(&marker.to_ascii_lowercase()) {
            return Err(anyhow!("LLM output contains refusal marker: \"{marker}\""));
        }
    }

    // --- Fabrication patterns ---
    // Small models sometimes generate fake commands or explanatory prose
    // instead of condensing the actual output.
    let fabrication_markers = [
        "this command will",
        "this will output",
        "this outputs",
        "the above command",
        "the following command",
        "you can use",
        "you can run",
        "to achieve this",
        "--rules=",         // fabricated flag
        "--remove-verbose", // fabricated flag
    ];

    for marker in &fabrication_markers {
        if lower.contains(&marker.to_ascii_lowercase()) {
            return Err(anyhow!(
                "LLM output contains fabrication marker: \"{marker}\""
            ));
        }
    }

    Ok(())
}

/// Detect when the LLM parrots the few-shot example instead of condensing
/// the actual input.
///
/// Small models sometimes reproduce the demonstration from the system prompt
/// verbatim. We compare the LLM output against the template's `example_after`
/// text (normalized) and reject if it's a near-exact match.
fn check_no_example_echo(command: &str, llm_output: &str) -> Result<()> {
    let example = super::prompts::example_after_for(command);
    check_no_example_echo_with(example, llm_output)
}

/// Inner implementation: compare `llm_output` against an expected `example`
/// string. Extracted so tests can call it directly without needing a live
/// template.
fn check_no_example_echo_with(example: &str, llm_output: &str) -> Result<()> {
    if example.is_empty() {
        return Ok(());
    }

    let normalize = |s: &str| -> String {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase()
    };

    let norm_example = normalize(example);
    let norm_llm = normalize(llm_output);

    // Exact match
    if norm_llm == norm_example {
        return Err(anyhow!(
            "LLM output is the few-shot example echoed back verbatim"
        ));
    }

    // The example is fully contained in the output (model padded it with
    // other content but still parroted it)
    if norm_example.len() > 10 && norm_llm.contains(&norm_example) {
        return Err(anyhow!(
            "LLM output contains the few-shot example echoed back"
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Preamble stripping
// ---------------------------------------------------------------------------

/// Preamble prefixes that small LLMs frequently add despite instructions
/// telling them to output raw data only. These are case-insensitively
/// matched against the first non-empty line of the LLM response.
const PREAMBLE_PREFIXES: &[&str] = &[
    "here is the condensed",
    "here's the condensed",
    "here is the optimized",
    "here's the optimized",
    "here is the summarized",
    "here's the summarized",
    "here is the summary",
    "here's the summary",
    "here is the output",
    "here's the output",
    "here is a condensed",
    "here's a condensed",
    "here are the",
    "sure, here",
    "sure! here",
    "certainly!",
    "certainly,",
    "of course!",
    "of course,",
];

/// Strip lines that look like shell commands from LLM output.
///
/// Small models sometimes prepend or insert command suggestions (e.g.
/// `git log --pretty=format:"%h %s"`) despite instructions not to.
/// Rather than rejecting the entire output, we strip obviously
/// command-shaped lines and keep the useful condensed content.
pub fn strip_command_lines(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let result: String = trimmed
        .lines()
        .filter(|line| !looks_like_command(line))
        .collect::<Vec<_>>()
        .join("\n");
    result.trim().to_string()
}

/// Heuristic: does this line look like a shell command rather than data?
fn looks_like_command(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();

    // Shell prompt indicators
    if lower.starts_with("$ ") || lower.starts_with("> ") || lower.starts_with("% ") {
        return true;
    }

    // Git commands with flags (e.g. `git log --pretty=format:...`)
    if lower.starts_with("git ") && (lower.contains(" --") || lower.contains(" -")) {
        return true;
    }

    // Bare `--pretty=format:` anywhere is a fabricated flag
    if lower.contains("--pretty=format:") {
        return true;
    }

    // Lines that are just a bare command invocation with pipes
    if lower.starts_with("git ") && lower.contains(" | ") {
        return true;
    }

    false
}

/// Strip common LLM conversational preamble from the beginning of a
/// response.
///
/// Small models often prepend phrases like "Here is the condensed output:"
/// despite being told not to. Instead of rejecting the whole response as
/// a hallucination, we strip the preamble and keep the useful content.
///
/// The function removes:
/// 1. Leading lines that match any [`PREAMBLE_PREFIXES`] entry.
/// 2. Markdown-style fenced code block wrappers (` ```\n ... \n``` `).
/// 3. Leading/trailing blank lines produced by the stripping.
pub fn strip_preamble(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut lines: Vec<&str> = trimmed.lines().collect();

    // Strip leading preamble line(s)
    while !lines.is_empty() {
        let first_lower = lines[0].trim().to_ascii_lowercase();
        if first_lower.is_empty() {
            lines.remove(0);
            continue;
        }
        let is_preamble = PREAMBLE_PREFIXES.iter().any(|p| first_lower.starts_with(p));
        if is_preamble {
            lines.remove(0);
            // Also skip a blank line immediately after the preamble
            if !lines.is_empty() && lines[0].trim().is_empty() {
                lines.remove(0);
            }
        } else {
            break;
        }
    }

    // Strip wrapping markdown fenced code blocks
    if lines.len() >= 2 {
        let first = lines[0].trim();
        let last = lines[lines.len() - 1].trim();
        if first.starts_with("```") && last == "```" {
            lines.remove(lines.len() - 1);
            lines.remove(0);
        }
    }

    let result: String = lines.join("\n");
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_condensed_output() {
        let raw = "This is a long command output with lots of unnecessary detail.";
        let llm = "Short summary.";
        assert!(validate_llm_output("whoami", raw, llm).is_ok());
    }

    #[test]
    fn rejects_empty_output() {
        let raw = "some output";
        assert!(validate_llm_output("whoami", raw, "").is_err());
        assert!(validate_llm_output("whoami", raw, "   ").is_err());
    }

    #[test]
    fn rejects_longer_output() {
        let raw = "short";
        let llm = "this is much much longer than the original output and clearly wrong";
        assert!(validate_llm_output("whoami", raw, llm).is_err());
    }

    #[test]
    fn allows_slightly_longer_within_margin() {
        let raw = "a]".repeat(50); // 100 chars
        let llm = "b".repeat(105); // 105 chars, within 10% margin of 100
        assert!(validate_llm_output("whoami", &raw, &llm).is_ok());
    }

    #[test]
    fn rejects_refusal_markers() {
        let raw = "Original output is reasonably long for the test to pass length checks.";
        let llm = "I apologize, here is the output.";
        assert!(validate_llm_output("whoami", raw, llm).is_err());

        let llm2 = "As an AI, I cannot determine the exact output.";
        assert!(validate_llm_output("whoami", raw, llm2).is_err());
    }

    #[test]
    fn rejects_refusal_responses() {
        let raw = "Original output is reasonably long for the test to pass length checks.";
        assert!(validate_llm_output("whoami", raw, "I can't fulfill this request.").is_err());
        assert!(validate_llm_output("whoami", raw, "I can't help with that.").is_err());
    }

    #[test]
    fn rejects_fabricated_commands() {
        let raw = "commit abc123\ncommit def456\ncommit ghi789 and more to be long enough.";
        let llm = "git log --rules=keep branch names\nThis command will output the commits.";
        assert!(validate_llm_output("git log", raw, llm).is_err());
    }

    #[test]
    fn rejects_fabricated_flags() {
        let raw = "commit abc123\ncommit def456\ncommit ghi789 and more to be long enough.";
        let llm = "git log --remove-verbose --oneline";
        assert!(validate_llm_output("git log", raw, llm).is_err());
    }

    #[test]
    fn accepts_output_without_markers() {
        let raw = "drwxr-xr-x  5 user staff  160 Jan 10 14:23 src and more detail here.";
        let llm = "src/ (dir)";
        assert!(validate_llm_output("ls -la", raw, llm).is_ok());
    }

    // -----------------------------------------------------------------------
    // Example-echo detection
    // -----------------------------------------------------------------------

    #[test]
    fn example_echo_check_is_noop_without_examples() {
        // Few-shot examples have been removed, so example_after_for returns ""
        // and the echo check is a harmless no-op.
        let raw =
            "On branch feature\nYour branch is up to date with origin/feature.\nnothing to commit";
        let llm = "branch: feature (up to date)";
        assert!(validate_llm_output("git status", raw, llm).is_ok());
    }

    #[test]
    fn echo_check_rejects_if_example_restored() {
        // Direct unit test: check_no_example_echo correctly catches parroting
        // even though the current templates don't include examples.
        let result = check_no_example_echo_with("some parroted text", "some parroted text");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Preamble stripping
    // -----------------------------------------------------------------------

    #[test]
    fn strip_preamble_removes_here_is() {
        let input = "Here is the condensed output:\n\nbranch: main\nmodified: src/main.rs";
        let result = strip_preamble(input);
        assert_eq!(result, "branch: main\nmodified: src/main.rs");
    }

    #[test]
    fn strip_preamble_removes_heres_the() {
        let input = "Here's the condensed version:\n\ncommit abc123 fix bug";
        let result = strip_preamble(input);
        assert_eq!(result, "commit abc123 fix bug");
    }

    #[test]
    fn strip_preamble_removes_sure_here() {
        let input = "Sure, here is the result:\n\nok 5 tests";
        let result = strip_preamble(input);
        assert_eq!(result, "ok 5 tests");
    }

    #[test]
    fn strip_preamble_removes_certainly() {
        let input = "Certainly!\n\nok 10 tests";
        let result = strip_preamble(input);
        assert_eq!(result, "ok 10 tests");
    }

    #[test]
    fn strip_preamble_removes_fenced_code_blocks() {
        let input = "```\nbranch: main\nmodified: src/main.rs\n```";
        let result = strip_preamble(input);
        assert_eq!(result, "branch: main\nmodified: src/main.rs");
    }

    #[test]
    fn strip_preamble_removes_preamble_and_fenced_code() {
        let input = "Here is the condensed output:\n\n```\nbranch: main\n```";
        let result = strip_preamble(input);
        assert_eq!(result, "branch: main");
    }

    #[test]
    fn strip_preamble_noop_on_clean_output() {
        let input = "branch: main\nmodified: src/main.rs";
        let result = strip_preamble(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_preamble_handles_empty() {
        assert_eq!(strip_preamble(""), "");
        assert_eq!(strip_preamble("   "), "");
    }

    // -----------------------------------------------------------------------
    // Command-line stripping
    // -----------------------------------------------------------------------

    #[test]
    fn strip_command_lines_removes_git_with_flags() {
        let input = "git log --pretty=format:\"%h %s\"\nabc1234 Fix bug\ndef5678 Add feature";
        let result = strip_command_lines(input);
        assert_eq!(result, "abc1234 Fix bug\ndef5678 Add feature");
    }

    #[test]
    fn strip_command_lines_removes_prompt_indicator() {
        let input = "$ git log --oneline\nabc1234 Fix bug";
        let result = strip_command_lines(input);
        assert_eq!(result, "abc1234 Fix bug");
    }

    #[test]
    fn strip_command_lines_removes_pretty_format_anywhere() {
        let input = "Use --pretty=format:\"%h\" to see hashes\nabc1234 Fix bug";
        let result = strip_command_lines(input);
        assert_eq!(result, "abc1234 Fix bug");
    }

    #[test]
    fn strip_command_lines_preserves_data_lines() {
        let input = "abc1234 Fix bug\ndef5678 Add feature\nghi9012 Merged PR 123";
        let result = strip_command_lines(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_command_lines_preserves_git_in_commit_messages() {
        // "git" at the start without flags is data, not a command
        let input = "abc1234 Fix bug\ngit integration tests pass now";
        let result = strip_command_lines(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_command_lines_handles_empty() {
        assert_eq!(strip_command_lines(""), "");
        assert_eq!(strip_command_lines("   "), "");
    }
}
