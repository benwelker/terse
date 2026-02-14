//! Truncation — intelligent size-limited truncation with context preservation.
//!
//! Stage 4 of the preprocessing pipeline. If the output still exceeds a
//! configurable maximum after noise removal, path filtering, and
//! deduplication, this module truncates the middle while preserving the head
//! (command start, errors at top) and tail (summary lines, final status).

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Fraction of the budget allocated to the head (beginning of output).
const HEAD_RATIO: f64 = 0.4;

/// Fraction of the budget allocated to the tail (end of output).
const TAIL_RATIO: f64 = 0.4;

// The remaining 20% is reserved for the truncation marker itself.

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Truncate text to fit within `max_bytes`, preserving head and tail.
///
/// If text is already within budget, it is returned unchanged. Otherwise,
/// the middle is removed and replaced with a marker indicating how many
/// lines/bytes were omitted.
pub fn truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let lines: Vec<&str> = text.lines().collect();

    // If we have very few lines, just byte-slice instead of line-slicing
    if lines.len() <= 6 {
        return byte_truncate(text, max_bytes);
    }

    let head_budget = (max_bytes as f64 * HEAD_RATIO) as usize;
    let tail_budget = (max_bytes as f64 * TAIL_RATIO) as usize;

    // Collect head lines
    let mut head_lines: Vec<&str> = Vec::new();
    let mut head_bytes = 0;
    for line in &lines {
        let line_cost = line.len() + 1; // +1 for newline
        if head_bytes + line_cost > head_budget {
            break;
        }
        head_lines.push(line);
        head_bytes += line_cost;
    }

    // Collect tail lines (from the end)
    let mut tail_lines: Vec<&str> = Vec::new();
    let mut tail_bytes = 0;
    for line in lines.iter().rev() {
        let line_cost = line.len() + 1;
        if tail_bytes + line_cost > tail_budget {
            break;
        }
        tail_lines.push(line);
        tail_bytes += line_cost;
    }
    tail_lines.reverse();

    // Ensure head and tail don't overlap
    let head_count = head_lines.len();
    let tail_start = lines.len().saturating_sub(tail_lines.len());
    if head_count >= tail_start {
        // Overlap — just do byte truncation
        return byte_truncate(text, max_bytes);
    }

    let omitted_lines = tail_start - head_count;
    let omitted_bytes: usize = lines[head_count..tail_start]
        .iter()
        .map(|l| l.len() + 1)
        .sum();

    let mut result = String::with_capacity(max_bytes);
    for line in &head_lines {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!(
        "\n[... {omitted_lines} lines ({omitted_bytes} bytes) truncated ...]\n\n"
    ));
    for line in &tail_lines {
        result.push_str(line);
        result.push('\n');
    }

    result
}

/// Simple byte-level truncation for very short texts.
fn byte_truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let half = max_bytes / 2;
    let head = &text[..half];
    let tail = &text[text.len() - half..];
    let omitted = text.len() - (2 * half);

    format!("{head}\n[... {omitted} bytes truncated ...]\n{tail}")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_when_within_budget() {
        let input = "short text\n";
        let result = truncate(input, 1024);
        assert_eq!(result, input);
    }

    #[test]
    fn truncates_large_output() {
        let mut input = String::new();
        for i in 0..1000 {
            input.push_str(&format!("line {i}: some content here for testing purposes\n"));
        }

        let max_bytes = 2048;
        let result = truncate(&input, max_bytes);

        // Result should be within budget (roughly — marker adds a bit)
        assert!(
            result.len() <= max_bytes + 200,
            "Result {} bytes exceeds budget {} + margin",
            result.len(),
            max_bytes
        );
        // Should contain head lines
        assert!(result.contains("line 0:"));
        // Should contain tail lines
        assert!(result.contains("line 999:"));
        // Should contain truncation marker
        assert!(result.contains("truncated"));
    }

    #[test]
    fn preserves_head_and_tail() {
        let mut input = String::new();
        input.push_str("=== BUILD STARTED ===\n");
        for i in 0..200 {
            input.push_str(&format!("compiling module_{i}.rs\n"));
        }
        input.push_str("=== BUILD COMPLETE: 200 modules ===\n");

        let result = truncate(&input, 512);
        assert!(result.contains("BUILD STARTED"));
        assert!(result.contains("BUILD COMPLETE"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(truncate("", 1024), "");
    }

    #[test]
    fn single_line_over_budget() {
        let input = "a".repeat(500);
        let result = truncate(&input, 100);
        assert!(result.len() <= 200); // Some overhead for marker
        assert!(result.contains("truncated"));
    }

    #[test]
    fn marker_shows_omitted_count() {
        let mut input = String::new();
        for i in 0..100 {
            input.push_str(&format!("line {i}\n"));
        }

        let result = truncate(&input, 256);
        assert!(result.contains("lines"));
        assert!(result.contains("bytes"));
        assert!(result.contains("truncated"));
    }
}
