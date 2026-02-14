use anyhow::Result;

use crate::config::schema::GenericOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Generic whitespace optimizer
// ---------------------------------------------------------------------------

/// A generic post-processing optimizer applied as a fallback when no
/// specialized optimizer matches.
///
/// This optimizer handles universal whitespace cleanup:
/// - Collapse 3+ consecutive blank lines to max 2
/// - Trim trailing whitespace from each line
/// - Remove trailing blank lines from overall output
/// - Cap total line count with head/tail preservation
///
/// It is registered last in the optimizer registry so that it only fires
/// when no specialized optimizer (git, file, build, docker) matches.
/// It accepts any command that produces output above a minimum size.
pub struct GenericOptimizer {
    /// Minimum raw output size (in bytes) to bother optimizing.
    /// Small outputs aren't worth the overhead.
    min_size: usize,
    /// Maximum lines to keep in the output.
    max_lines: usize,
}

impl Default for GenericOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GenericOptimizer {
    pub fn new() -> Self {
        let cfg = GenericOptimizerConfig::default();
        Self {
            min_size: cfg.min_size_bytes,
            max_lines: cfg.max_lines,
        }
    }

    /// Create a `GenericOptimizer` from the configuration.
    pub fn from_config(cfg: &GenericOptimizerConfig) -> Self {
        Self {
            min_size: cfg.min_size_bytes,
            max_lines: cfg.max_lines,
        }
    }
}

impl Optimizer for GenericOptimizer {
    fn name(&self) -> &'static str {
        "generic"
    }

    /// The generic optimizer accepts any command whose raw output is large
    /// enough to benefit from whitespace cleanup.
    fn can_handle(&self, _ctx: &CommandContext) -> bool {
        // Always return true — the registry tries specialized optimizers first
        // and only reaches generic as a fallback. Actual size gating happens
        // in `optimize_output` where we check raw_output length.
        true
    }

    fn optimize_output(&self, _ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput> {
        // Skip tiny outputs — not worth optimizing
        if raw_output.len() < self.min_size {
            return Ok(OptimizedOutput {
                optimized_tokens: estimate_tokens(raw_output),
                output: raw_output.to_string(),
                optimizer_used: self.name().to_string(),
            });
        }

        let optimized = cleanup_whitespace(raw_output, self.max_lines);

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Whitespace cleanup
// ---------------------------------------------------------------------------

/// Apply universal whitespace cleanup to output text.
///
/// 1. Trim trailing whitespace from each line
/// 2. Collapse 3+ consecutive blank lines to max 2
/// 3. Remove trailing blank lines
/// 4. Cap total lines with head/tail preservation
pub fn cleanup_whitespace(text: &str, max_lines: usize) -> String {
    let mut result = Vec::new();
    let mut consecutive_blanks = 0u32;
    let max_consecutive_blanks = 2u32;

    for line in text.lines() {
        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= max_consecutive_blanks {
                result.push(String::new());
            }
        } else {
            consecutive_blanks = 0;
            result.push(trimmed.to_string());
        }
    }

    // Remove trailing blank lines
    while result.last().is_some_and(|l| l.is_empty()) {
        result.pop();
    }

    // Cap total lines
    let total = result.len();
    if total > max_lines {
        let head_count = max_lines * 2 / 3; // ~67% head
        let tail_count = max_lines - head_count - 1; // rest for tail + gap line

        let mut capped = Vec::with_capacity(max_lines);
        capped.extend_from_slice(&result[..head_count]);
        capped.push(format!(
            "\n... ({} lines omitted, {} total) ...\n",
            total - head_count - tail_count,
            total
        ));
        capped.extend_from_slice(&result[total - tail_count..]);
        return capped.join("\n");
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::CommandContext;

    // can_handle ---------------------------------------------------------

    #[test]
    fn generic_always_handles() {
        let opt = GenericOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("unknown_command")));
        assert!(opt.can_handle(&CommandContext::new("some-tool --verbose")));
    }

    // cleanup_whitespace -------------------------------------------------

    #[test]
    fn collapses_consecutive_blanks() {
        let input = "line 1\n\n\n\n\nline 2\n\n\nline 3";
        let result = cleanup_whitespace(input, 200);
        assert_eq!(result, "line 1\n\n\nline 2\n\n\nline 3");
    }

    #[test]
    fn trims_trailing_whitespace() {
        let input = "line 1   \nline 2\t\t\nline 3  ";
        let result = cleanup_whitespace(input, 200);
        assert_eq!(result, "line 1\nline 2\nline 3");
    }

    #[test]
    fn removes_trailing_blank_lines() {
        let input = "line 1\nline 2\n\n\n\n";
        let result = cleanup_whitespace(input, 200);
        assert_eq!(result, "line 1\nline 2");
    }

    #[test]
    fn caps_total_lines() {
        let lines: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
        let input = lines.join("\n");
        let result = cleanup_whitespace(&input, 100);
        let result_lines: Vec<&str> = result.lines().collect();
        // Should be around 100 lines (head + gap + tail)
        assert!(result_lines.len() <= 102, "got {} lines", result_lines.len());
        assert!(result.contains("lines omitted"));
        assert!(result.contains("line 0")); // head preserved
        assert!(result.contains("line 299")); // tail preserved
    }

    #[test]
    fn small_output_passthrough() {
        let opt = GenericOptimizer::new();
        let ctx = CommandContext::new("some-cmd");
        let small = "tiny output";
        let result = opt.optimize_output(&ctx, small).unwrap();
        assert_eq!(result.output, small);
    }

    #[test]
    fn preserves_short_output() {
        let input = "line 1\nline 2\nline 3";
        let result = cleanup_whitespace(input, 200);
        assert_eq!(result, input);
    }

    #[test]
    fn empty_input() {
        let result = cleanup_whitespace("", 200);
        assert_eq!(result, "");
    }
}
