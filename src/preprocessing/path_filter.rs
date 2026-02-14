//! Path filtering — collapse verbose directory/path noise into summary lines.
//!
//! Stage 2 of the preprocessing pipeline. Many command outputs (find, tree,
//! build logs, test runners) contain long lists of paths through directories
//! that are irrelevant to an AI coding assistant (e.g. `node_modules/`,
//! `.git/objects/`, `target/debug/deps/`). This module detects those paths
//! and replaces runs of them with a single summary line.

// ---------------------------------------------------------------------------
// Directories that are safe to filter from output
// ---------------------------------------------------------------------------

/// Path segments that indicate noise directories.
/// If any segment of a path matches one of these, the line is a candidate for
/// filtering.
const NOISE_DIR_SEGMENTS: &[&str] = &[
    "node_modules",
    ".git/objects",
    ".git/refs",
    ".git/logs",
    ".git/hooks",
    "target/debug/deps",
    "target/debug/build",
    "target/debug/incremental",
    "target/release/deps",
    "target/release/build",
    "target/release/incremental",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    "dist/",
    "build/lib",
    ".next/",
    ".nuxt/",
    ".cache/",
    "coverage/",
    ".nyc_output",
    "vendor/bundle",
    "Pods/",
    ".gradle/",
    "bin/Debug",
    "bin/Release",
    "obj/Debug",
    "obj/Release",
    ".vs/",
    ".idea/",
];

/// Returns `true` if a line looks like a file-path line belonging to a noise
/// directory.
///
/// Heuristic: the line (after trimming) is either:
/// - A file path containing one of the noise segments, OR
/// - A tree-style listing (e.g. `│   ├── node_modules/...`)
fn is_noise_path_line(line: &str) -> bool {
    let trimmed = line.trim();

    // Skip empty lines — not path lines
    if trimmed.is_empty() {
        return false;
    }

    // Strip tree-drawing characters (│ ├ └ ─ etc.) to get the bare path
    let bare: String = trimmed
        .chars()
        .filter(|c| !matches!(c, '│' | '├' | '└' | '─' | '┬' | '┤' | '┌' | '┐' | '┘' | '┴'))
        .collect();
    let bare = bare.trim();

    // Normalize backslashes → forward slashes for matching
    let normalized = bare.replace('\\', "/");

    NOISE_DIR_SEGMENTS
        .iter()
        .any(|seg| normalized.contains(seg))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Filter noise directory paths from the output.
///
/// Consecutive lines that reference noise directories are collapsed into a
/// single summary line. Non-noise lines pass through unchanged.
pub fn filter_paths(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result = String::with_capacity(text.len());

    let mut i = 0;
    while i < lines.len() {
        if is_noise_path_line(lines[i]) {
            // Count consecutive noise lines
            let start = i;
            while i < lines.len() && is_noise_path_line(lines[i]) {
                i += 1;
            }
            let count = i - start;
            // Emit a single summary line
            result.push_str(&format!(
                "[{count} path(s) in noise directories filtered]\n"
            ));
        } else {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
        }
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
    fn filters_node_modules_paths() {
        let input = "\
src/main.rs
node_modules/serde/lib.rs
node_modules/serde_json/lib.rs
node_modules/anyhow/lib.rs
src/lib.rs
";
        let result = filter_paths(input);
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("src/lib.rs"));
        assert!(!result.contains("node_modules/serde"));
        assert!(result.contains("[3 path(s) in noise directories filtered]"));
    }

    #[test]
    fn filters_target_debug_deps() {
        let input = "\
Cargo.toml
target/debug/deps/serde-abc123.d
target/debug/deps/anyhow-def456.d
target/debug/deps/clap-ghi789.d
src/main.rs
";
        let result = filter_paths(input);
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("src/main.rs"));
        assert!(!result.contains("target/debug/deps"));
        assert!(result.contains("[3 path(s) in noise directories filtered]"));
    }

    #[test]
    fn filters_git_objects() {
        let input = "\
.git/objects/ab/cdef1234567890
.git/objects/cd/ef1234567890ab
README.md
";
        let result = filter_paths(input);
        assert!(!result.contains(".git/objects"));
        assert!(result.contains("README.md"));
        assert!(result.contains("[2 path(s) in noise directories filtered]"));
    }

    #[test]
    fn preserves_non_noise_paths() {
        let input = "\
src/main.rs
src/lib.rs
tests/integration_test.rs
";
        let result = filter_paths(input);
        assert!(result.contains("src/main.rs"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("tests/integration_test.rs"));
        assert!(!result.contains("filtered"));
    }

    #[test]
    fn handles_windows_backslash_paths() {
        let input = "\
src\\main.rs
target\\debug\\deps\\serde-abc.d
target\\debug\\deps\\clap-xyz.d
Cargo.toml
";
        let result = filter_paths(input);
        assert!(result.contains("src\\main.rs"));
        assert!(result.contains("Cargo.toml"));
        assert!(!result.contains("target\\debug\\deps"));
        assert!(result.contains("[2 path(s) in noise directories filtered]"));
    }

    #[test]
    fn handles_tree_style_output() {
        let input = "\
├── src/
│   ├── main.rs
│   └── lib.rs
├── node_modules/
│   ├── node_modules/serde/lib.rs
│   └── node_modules/anyhow/lib.rs
└── Cargo.toml
";
        let result = filter_paths(input);
        assert!(result.contains("main.rs"));
        assert!(result.contains("Cargo.toml"));
        assert!(!result.contains("node_modules/serde"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(filter_paths(""), "");
    }

    #[test]
    fn multiple_separate_noise_runs() {
        let input = "\
src/main.rs
node_modules/a/b
node_modules/c/d
src/lib.rs
target/debug/deps/x.d
target/debug/deps/y.d
Cargo.toml
";
        let result = filter_paths(input);
        // Two separate filtered blocks
        let filter_count = result.matches("filtered]").count();
        assert_eq!(filter_count, 2);
    }
}
