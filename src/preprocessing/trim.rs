//! Trim — final whitespace normalization.
//!
//! Stage 5 (final) of the preprocessing pipeline. Ensures consistent
//! whitespace output regardless of what previous stages produced:
//!
//! - Trim leading/trailing whitespace from the full output.
//! - Collapse runs of 3+ blank lines down to at most 2.
//! - Strip trailing whitespace from each line.

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Normalize whitespace in the preprocessed text.
///
/// This is the final pipeline stage — it cleans up any whitespace artifacts
/// introduced by previous stages.
pub fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut consecutive_blanks: usize = 0;

    for line in text.lines() {
        let trimmed_end = line.trim_end();

        if trimmed_end.is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 2 {
                result.push('\n');
            }
            // Skip if we already have 2 blank lines
        } else {
            consecutive_blanks = 0;
            result.push_str(trimmed_end);
            result.push('\n');
        }
    }

    // Trim leading and trailing whitespace from the entire output
    let trimmed = result.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_leading_and_trailing_whitespace() {
        let input = "\n\n\n  hello world  \n\n\n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn collapses_multiple_blank_lines() {
        let input = "line 1\n\n\n\n\n\nline 2\n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line 1\n\n\nline 2");
    }

    #[test]
    fn strips_trailing_whitespace_per_line() {
        let input = "hello   \nworld   \n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn preserves_single_blank_lines() {
        let input = "line 1\n\nline 2\n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line 1\n\nline 2");
    }

    #[test]
    fn preserves_two_blank_lines() {
        let input = "line 1\n\n\nline 2\n";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line 1\n\n\nline 2");
    }

    #[test]
    fn empty_input() {
        assert_eq!(normalize_whitespace(""), "");
    }

    #[test]
    fn only_whitespace() {
        assert_eq!(normalize_whitespace("   \n\n\n   \n"), "");
    }

    #[test]
    fn mixed_whitespace_issues() {
        let input = "\n\nhello   \n\n\n\n\nworld  \n  trailing  \n\n\n\n";
        let result = normalize_whitespace(input);
        // At most 2 consecutive blanks; trailing whitespace stripped
        assert!(result.starts_with("hello"));
        assert!(result.contains("world"));
        assert!(result.contains("trailing"));
        // No trailing whitespace on any line
        for line in result.lines() {
            assert_eq!(line, line.trim_end());
        }
    }
}
