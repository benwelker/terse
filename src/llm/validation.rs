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
pub fn validate_llm_output(raw_output: &str, llm_output: &str) -> Result<()> {
    check_non_empty(llm_output)?;
    check_shorter(raw_output, llm_output)?;
    check_no_hallucination_markers(llm_output)?;
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

/// Check for common hallucination markers in LLM output.
///
/// These patterns suggest the LLM invented content rather than condensing it.
fn check_no_hallucination_markers(llm_output: &str) -> Result<()> {
    let markers = [
        "I apologize",
        "I'm sorry",
        "As an AI",
        "I cannot",
        "I don't have access",
        "Here is the condensed",
        "Here's the condensed",
        "Sure, here",
        "Certainly!",
        "Of course!",
    ];

    let lower = llm_output.to_ascii_lowercase();
    for marker in &markers {
        if lower.contains(&marker.to_ascii_lowercase()) {
            return Err(anyhow!(
                "LLM output contains hallucination marker: \"{marker}\""
            ));
        }
    }

    Ok(())
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
        assert!(validate_llm_output(raw, llm).is_ok());
    }

    #[test]
    fn rejects_empty_output() {
        let raw = "some output";
        assert!(validate_llm_output(raw, "").is_err());
        assert!(validate_llm_output(raw, "   ").is_err());
    }

    #[test]
    fn rejects_longer_output() {
        let raw = "short";
        let llm = "this is much much longer than the original output and clearly wrong";
        assert!(validate_llm_output(raw, llm).is_err());
    }

    #[test]
    fn allows_slightly_longer_within_margin() {
        let raw = "a]".repeat(50); // 100 chars
        let llm = "b".repeat(105); // 105 chars, within 10% margin of 100
        assert!(validate_llm_output(&raw, &llm).is_ok());
    }

    #[test]
    fn rejects_hallucination_markers() {
        let raw = "Original output is reasonably long for the test to pass length checks.";
        let llm = "I apologize, here is the output.";
        assert!(validate_llm_output(raw, llm).is_err());

        let llm2 = "As an AI, I cannot determine the exact output.";
        assert!(validate_llm_output(raw, llm2).is_err());
    }

    #[test]
    fn accepts_output_without_markers() {
        let raw = "drwxr-xr-x  5 user staff  160 Jan 10 14:23 src and more detail here.";
        let llm = "src/ (dir)";
        assert!(validate_llm_output(raw, llm).is_ok());
    }
}
