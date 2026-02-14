//! Deduplication — collapse repeated or near-identical consecutive lines.
//!
//! Stage 3 of the preprocessing pipeline. Test runners, build tools, and
//! linters often emit hundreds of structurally identical lines (e.g.
//! `test foo::bar ... ok`). This module detects consecutive runs of similar
//! lines and collapses them into a counted summary.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Minimum number of consecutive similar lines to trigger deduplication.
const MIN_RUN_LENGTH: usize = 3;

/// Maximum number of unique lines to show before collapsing a run.
/// The first and last representative lines are always kept.
const REPRESENTATIVE_LINES: usize = 2;

// ---------------------------------------------------------------------------
// Line similarity
// ---------------------------------------------------------------------------

/// Extract a "pattern key" from a line for grouping.
///
/// The key removes variable parts (numbers, hashes, UUIDs, timestamps) so
/// that structurally identical lines share the same key.
///
/// Examples:
/// - `"test foo::bar_123 ... ok"` → `"test ...::... ... ok"`
/// - `"  PASS src/tests/test_01.rs"` → `"PASS src/tests/....rs"`
fn pattern_key(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut key = String::with_capacity(trimmed.len());
    let mut chars = trimmed.chars().peekable();

    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            // Skip consecutive digits — replace the run with a single `#`
            while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                chars.next();
            }
            key.push('#');
        } else if c.is_ascii_hexdigit() && key.ends_with('#') {
            // Part of a hex sequence after digits — skip
            while chars
                .peek()
                .is_some_and(|ch| ch.is_ascii_hexdigit())
            {
                chars.next();
            }
        } else {
            key.push(c);
        }
    }

    key
}

// ---------------------------------------------------------------------------
// Run detection and collapsing
// ---------------------------------------------------------------------------

/// A run of consecutive lines sharing the same pattern key.
struct Run<'a> {
    key: String,
    lines: Vec<&'a str>,
}

/// Detect runs of similar consecutive lines.
fn detect_runs(text: &str) -> Vec<Run<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut runs: Vec<Run<'_>> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let key = pattern_key(lines[i]);
        let mut run_lines = vec![lines[i]];

        // Collect consecutive lines with the same key
        while i + run_lines.len() < lines.len() {
            let next_key = pattern_key(lines[i + run_lines.len()]);
            if next_key == key && !key.is_empty() {
                run_lines.push(lines[i + run_lines.len()]);
            } else {
                break;
            }
        }

        i += run_lines.len();
        runs.push(Run {
            key,
            lines: run_lines,
        });
    }

    runs
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Deduplicate consecutive similar lines in the text.
///
/// Runs of `MIN_RUN_LENGTH` or more similar lines are collapsed into a
/// summary showing the count and representative examples. Short runs and
/// unique lines pass through unchanged.
pub fn deduplicate(text: &str) -> String {
    let runs = detect_runs(text);
    let mut result = String::with_capacity(text.len());

    for run in &runs {
        let count = run.lines.len();

        if count < MIN_RUN_LENGTH || run.key.is_empty() {
            // Short run or blank lines — emit unchanged
            for line in &run.lines {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            // Collapse the run
            // Show first representative line(s)
            let show_count = REPRESENTATIVE_LINES.min(count);
            for line in run.lines.iter().take(show_count) {
                result.push_str(line);
                result.push('\n');
            }
            if count > show_count {
                let remaining = count - show_count;
                // Summarize what was common about the run
                let sample = run.lines[0].trim();
                let prefix = common_prefix(sample);
                result.push_str(&format!(
                    "[... {remaining} more similar line(s) matching \"{prefix}...\"]\n"
                ));
            }
        }
    }

    result
}

/// Extract a short prefix from a line for display in summaries.
fn common_prefix(line: &str) -> String {
    let max_len = 40;
    if line.len() <= max_len {
        line.to_string()
    } else {
        line[..max_len].to_string()
    }
}

/// Produce a frequency summary of repeated non-consecutive lines.
///
/// This is a secondary deduplication pass that counts lines appearing many
/// times anywhere in the text (not just consecutively). Returns a map of
/// `line → count` for lines appearing more than `threshold` times.
#[allow(dead_code)]
pub fn frequency_summary(text: &str, threshold: usize) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() {
            *counts.entry(trimmed).or_insert(0) += 1;
        }
    }
    counts.retain(|_, count| *count > threshold);
    counts
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_repeated_test_lines() {
        let mut input = String::new();
        for i in 0..20 {
            input.push_str(&format!("test tests::test_{i} ... ok\n"));
        }

        let result = deduplicate(&input);
        // Should show first 2 lines + summary
        assert!(result.contains("test tests::test_0 ... ok"));
        assert!(result.contains("test tests::test_1 ... ok"));
        assert!(result.contains("18 more similar"));
        // Should NOT contain all 20 lines
        assert!(!result.contains("test tests::test_19 ... ok"));
    }

    #[test]
    fn preserves_short_runs() {
        let input = "line a\nline b\n";
        let result = deduplicate(input);
        assert!(result.contains("line a"));
        assert!(result.contains("line b"));
        assert!(!result.contains("similar"));
    }

    #[test]
    fn preserves_unique_lines() {
        let input = "error: type mismatch\nwarning: unused variable\nnote: see docs\n";
        let result = deduplicate(input);
        assert!(result.contains("error: type mismatch"));
        assert!(result.contains("warning: unused variable"));
        assert!(result.contains("note: see docs"));
    }

    #[test]
    fn collapses_pass_lines() {
        let mut input = String::new();
        for i in 0..50 {
            input.push_str(&format!("  PASS src/tests/test_{i:02}.rs\n"));
        }

        let result = deduplicate(&input);
        assert!(result.contains("PASS"));
        assert!(result.contains("more similar"));
        // Much shorter than original
        assert!(result.len() < input.len() / 2);
    }

    #[test]
    fn handles_mixed_content() {
        let mut input = String::new();
        input.push_str("Building project...\n");
        for i in 0..10 {
            input.push_str(&format!("test mod::test_{i} ... ok\n"));
        }
        input.push_str("test result: ok. 10 passed\n");
        input.push_str("error: something failed\n");

        let result = deduplicate(&input);
        assert!(result.contains("Building project..."));
        assert!(result.contains("test result: ok. 10 passed"));
        assert!(result.contains("error: something failed"));
        assert!(result.contains("more similar"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(deduplicate(""), "");
    }

    #[test]
    fn pattern_key_normalizes_numbers() {
        let k1 = pattern_key("test tests::test_0 ... ok");
        let k2 = pattern_key("test tests::test_19 ... ok");
        assert_eq!(k1, k2);
    }

    #[test]
    fn frequency_summary_counts() {
        let input = "ok\nok\nok\nfail\nok\n";
        let summary = frequency_summary(input, 2);
        assert_eq!(summary.get("ok"), Some(&4));
        assert!(!summary.contains_key("fail"));
    }
}
