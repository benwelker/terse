//! Preprocessing pipeline for the Smart Path.
//!
//! Reduces raw command output by 40–70% *before* sending it to the LLM,
//! improving quality, latency, and token efficiency. The pipeline is
//! deterministic, fast (<5 ms for 100 KB input), and runs as a stage between
//! raw command execution and LLM optimization.
//!
//! # Pipeline Stages
//!
//! 1. **Noise removal** — strip ANSI escape codes, progress bars, spinner
//!    frames, and common boilerplate lines.
//! 2. **Path filtering** — collapse verbose directory listings (node_modules,
//!    .git, target, etc.) into summary lines.
//! 3. **Deduplication** — collapse repeated or near-identical consecutive
//!    lines into counted summaries.
//! 4. **Truncation** — if output still exceeds a configurable max size, keep
//!    the head and tail with a middle-truncation marker.
//! 5. **Trim** — normalize whitespace: collapse runs of blank lines, strip
//!    trailing whitespace, trim leading/trailing.

pub mod dedup;
pub mod noise;
pub mod path_filter;
pub mod trim;
pub mod truncation;

// ---------------------------------------------------------------------------
// Pipeline output
// ---------------------------------------------------------------------------

/// Result of the preprocessing pipeline.
#[derive(Debug, Clone)]
pub struct PreprocessedOutput {
    /// The preprocessed text, ready for LLM consumption.
    pub text: String,
    /// Bytes in the original raw input.
    #[allow(dead_code)]
    pub original_bytes: usize,
    /// Bytes removed by preprocessing.
    pub bytes_removed: usize,
    /// Percentage of bytes removed (0.0–100.0).
    pub reduction_pct: f64,
}

// ---------------------------------------------------------------------------
// Pipeline orchestrator
// ---------------------------------------------------------------------------

/// Default maximum output size after preprocessing (bytes).
/// If the output still exceeds this after noise/path/dedup, truncation kicks in.
const DEFAULT_MAX_OUTPUT_BYTES: usize = 32 * 1024; // 32 KB

/// Run the full preprocessing pipeline on raw command output.
///
/// Each stage is applied in order. The pipeline is infallible — if any stage
/// encounters unexpected input it returns the text unchanged.
///
/// `command` is provided for context-aware decisions (e.g. path filtering
/// heuristics) but may be unused in early stages.
pub fn preprocess(raw: &str, _command: &str) -> PreprocessedOutput {
    preprocess_with_max(raw, _command, DEFAULT_MAX_OUTPUT_BYTES)
}

/// Run the preprocessing pipeline with a custom max output size (bytes).
///
/// Primarily used by tests.
pub fn preprocess_with_max(raw: &str, _command: &str, max_bytes: usize) -> PreprocessedOutput {
    let original_bytes = raw.len();

    // Stage 1: noise removal
    let text = noise::strip_noise(raw);

    // Stage 2: path filtering
    let text = path_filter::filter_paths(&text);

    // Stage 3: deduplication
    let text = dedup::deduplicate(&text);

    // Stage 4: truncation (only if still over budget)
    let text = truncation::truncate(&text, max_bytes);

    // Stage 5: trim / whitespace normalization
    let text = trim::normalize_whitespace(&text);

    let bytes_removed = original_bytes.saturating_sub(text.len());
    let reduction_pct = if original_bytes == 0 {
        0.0
    } else {
        (bytes_removed as f64 / original_bytes as f64) * 100.0
    };

    PreprocessedOutput {
        text,
        original_bytes,
        bytes_removed,
        reduction_pct,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        let result = preprocess("", "test");
        assert!(result.text.is_empty());
        assert_eq!(result.original_bytes, 0);
        assert_eq!(result.bytes_removed, 0);
    }

    #[test]
    fn small_clean_input_is_mostly_unchanged() {
        let input = "hello world\n";
        let result = preprocess(input, "echo hello");
        assert_eq!(result.text, "hello world");
        assert!(result.reduction_pct < 20.0);
    }

    #[test]
    fn pipeline_processes_complex_input() {
        // Combine ANSI codes + repeated lines + trailing whitespace
        let mut input = String::new();
        input.push_str("\x1b[32m   Compiling\x1b[0m serde v1.0.200\n");
        input.push_str("\x1b[32m   Compiling\x1b[0m serde_json v1.0.100\n");
        for i in 0..100 {
            input.push_str(&format!("test tests::test_{i} ... ok\n"));
        }
        input.push_str("\ntest result: ok. 100 passed; 0 failed\n");

        let result = preprocess(&input, "cargo test");
        // Should be significantly smaller
        assert!(
            result.reduction_pct > 30.0,
            "Expected >30% reduction, got {:.1}%",
            result.reduction_pct
        );
        // Must preserve the final summary
        assert!(
            result.text.contains("100 passed"),
            "Summary line must survive preprocessing"
        );
    }
}
