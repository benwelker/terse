//! Noise removal — strip ANSI escape codes, progress bars, spinner frames,
//! and common boilerplate lines from raw command output.
//!
//! This is Stage 1 of the preprocessing pipeline and typically provides the
//! largest byte-reduction for build/test outputs.

use std::borrow::Cow;

use regex::Regex;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Compiled regexes (compiled once, reused)
// ---------------------------------------------------------------------------

/// Matches ANSI escape sequences: CSI sequences `\x1b[...X` and OSC
/// sequences `\x1b]...ST`.
static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    // CSI: \x1b[ followed by parameter bytes (0x30-0x3f)*, intermediate
    // bytes (0x20-0x2f)*, and a final byte (0x40-0x7e).
    // OSC: \x1b] ... (ST = \x1b\\ or \x07).
    // Also matches \x1b followed by a single character (e.g. \x1b(B).
    Regex::new(r"\x1b\[[0-9;]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][A-B0-2]")
        .expect("ANSI regex must compile")
});

/// Matches common progress indicators:
/// - Percentage patterns like `73%`, `[====>   ]`, `[####    ]`
/// - Spinner characters
/// - Cargo/npm download progress lines
static PROGRESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
          # Bracket-delimited progress bars: [====>   ] or [####    ]
          \[[\s=>\#-]+\]
          |
          # Percentage with optional prefix: 73% or  52.3%
          \b\d{1,3}(?:\.\d+)?%
        ",
    )
    .expect("progress regex must compile")
});

// ---------------------------------------------------------------------------
// Boilerplate patterns (matched as line prefixes or exact content)
// ---------------------------------------------------------------------------

/// Line prefixes that indicate Cargo/npm/yarn compilation or download noise.
/// The line is removed entirely if it starts with one of these (after
/// whitespace trimming).
const BOILERPLATE_PREFIXES: &[&str] = &[
    "Compiling ",
    "Downloading ",
    "Downloaded ",
    "Checking ",
    "Fresh ",
    "Blocking waiting for file lock",
    "Updating crates.io index",
    "Unpacking ",
    "Resolving ",
    "Installing ",
    "Auditing ",
    "npm warn",
    "npm notice",
    "added ",      // "added 120 packages in 5s"
    "removed ",    // "removed 3 packages in 1s"
    "changed ",    // "changed 4 packages in 2s"
    "up to date,", // "up to date, audited 300 packages"
];

/// Lines that consist entirely of one repeated character (e.g. `=====` or `-----`)
/// are decoration noise.
fn is_decoration_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let mut chars = trimmed.chars();
    let first = chars.next().unwrap();
    // Only treat punctuation/symbol repeats as decoration
    if !first.is_ascii_punctuation() {
        return false;
    }
    chars.all(|c| c == first)
}

/// Returns true if the line is a progress/download line that should be removed.
fn is_progress_line(line: &str) -> bool {
    let trimmed = line.trim();

    // Carriage-return lines (overwritten progress)
    if trimmed.contains('\r') && !trimmed.ends_with('\r') {
        return true;
    }

    // Fast exit: progress patterns require '%' or '['. Skip the regex
    // entirely when neither character is present.
    if !trimmed.contains('%') && !trimmed.contains('[') {
        return false;
    }

    // Lines that are *only* a progress bar (no other meaningful content)
    if PROGRESS_RE.is_match(trimmed) && trimmed.len() < 120 {
        // Heuristic: if the line is short and dominated by the progress
        // pattern it's probably a progress indicator, not real output.
        let without_progress = PROGRESS_RE.replace_all(trimmed, "");
        if without_progress.trim().len() < 10 {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Strip noise from raw command output.
///
/// Removes ANSI escape codes, progress bars, boilerplate compilation/download
/// lines, and decoration separators. Returns the cleaned text.
pub fn strip_noise(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());

    for line in raw.lines() {
        // Step 1: strip ANSI escape sequences — only run the regex when the
        // line actually contains an ESC byte. For most command outputs (git
        // log, build output) the vast majority of lines contain no ANSI
        // codes, and a byte scan is orders of magnitude cheaper than
        // executing the regex automaton.
        let clean: Cow<'_, str> = if line.contains('\x1b') {
            ANSI_RE.replace_all(line, "")
        } else {
            Cow::Borrowed(line)
        };
        let trimmed = clean.trim_start();

        // Step 2: skip boilerplate lines
        if BOILERPLATE_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            continue;
        }

        // Step 3: skip decoration lines
        if is_decoration_line(&clean) {
            continue;
        }

        // Step 4: skip progress-only lines
        if is_progress_line(&clean) {
            continue;
        }

        result.push_str(&clean);
        result.push('\n');
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_escape_codes() {
        let input = "\x1b[32m   Compiling\x1b[0m serde v1.0.200\n\x1b[1;31merror\x1b[0m: failed";
        let result = strip_noise(input);
        assert!(!result.contains("\x1b["));
        // "Compiling" boilerplate is also stripped, but "error" should remain
        assert!(result.contains("error"), "error line must survive");
    }

    #[test]
    fn strips_boilerplate_cargo_lines() {
        let input = "\
   Compiling serde v1.0.200
   Compiling serde_json v1.0.100
   Compiling terse v0.1.0
running 10 tests
test result: ok. 10 passed; 0 failed
";
        let result = strip_noise(input);
        assert!(!result.contains("Compiling"));
        assert!(result.contains("running 10 tests"));
        assert!(result.contains("10 passed"));
    }

    #[test]
    fn strips_npm_boilerplate() {
        let input = "\
npm warn deprecated rimraf@3.0.0
npm notice New major version of npm available!
added 120 packages in 5s
up to date, audited 300 packages in 2s
Found 0 vulnerabilities
";
        let result = strip_noise(input);
        assert!(!result.contains("npm warn"));
        assert!(!result.contains("npm notice"));
        assert!(!result.contains("added 120"));
        assert!(result.contains("vulnerabilities"));
    }

    #[test]
    fn strips_decoration_lines() {
        let input = "Header\n====================\nContent\n--------------------\nFooter\n";
        let result = strip_noise(input);
        assert!(!result.contains("====="));
        assert!(!result.contains("-----"));
        assert!(result.contains("Header"));
        assert!(result.contains("Content"));
        assert!(result.contains("Footer"));
    }

    #[test]
    fn preserves_error_lines_with_ansi() {
        let input = "\x1b[1;31merror[E0308]\x1b[0m: mismatched types\n  --> src/main.rs:10:5\n";
        let result = strip_noise(input);
        assert!(result.contains("error[E0308]"));
        assert!(result.contains("src/main.rs"));
    }

    #[test]
    fn preserves_plain_text() {
        let input = "This is just normal output\nwith multiple lines\n";
        let result = strip_noise(input);
        assert!(result.contains("This is just normal output"));
        assert!(result.contains("with multiple lines"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(strip_noise(""), "");
    }
}
