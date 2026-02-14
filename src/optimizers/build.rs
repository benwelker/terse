use anyhow::Result;

use crate::config::schema::BuildOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Subcommand classification
// ---------------------------------------------------------------------------

/// Recognized build/test tool commands that terse can optimize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildCommand {
    /// cargo test / npm test / dotnet test / pytest / go test
    Test,
    /// cargo build / npm install / dotnet build / go build / make
    Build,
    /// cargo clippy / eslint / dotnet format
    Lint,
}

/// Classify the core command into a [`BuildCommand`].
fn classify(lower: &str) -> Option<BuildCommand> {
    // Test commands
    if lower.starts_with("cargo test")
        || lower.starts_with("npm test")
        || lower.starts_with("npm run test")
        || lower.starts_with("npx jest")
        || lower.starts_with("npx vitest")
        || lower.starts_with("dotnet test")
        || lower.starts_with("pytest")
        || lower.starts_with("python -m pytest")
        || lower.starts_with("go test")
        || lower.starts_with("mvn test")
        || lower.starts_with("gradle test")
        || lower.starts_with("make test")
        || lower.starts_with("nmake test")
    {
        return Some(BuildCommand::Test);
    }

    // Build/install commands
    if lower.starts_with("cargo build")
        || lower.starts_with("cargo install")
        || lower.starts_with("npm install")
        || lower.starts_with("npm ci")
        || lower.starts_with("npm run build")
        || lower.starts_with("npx tsc")
        || lower.starts_with("yarn install")
        || lower.starts_with("yarn build")
        || lower.starts_with("pnpm install")
        || lower.starts_with("pnpm build")
        || lower.starts_with("dotnet build")
        || lower.starts_with("dotnet restore")
        || lower.starts_with("dotnet publish")
        || lower.starts_with("go build")
        || lower.starts_with("mvn compile")
        || lower.starts_with("mvn package")
        || lower.starts_with("gradle build")
        || lower.starts_with("make")
        || lower.starts_with("cmake")
        || lower.starts_with("msbuild")
        || lower.starts_with("nmake")
        || lower.starts_with("nuget restore")
        || lower.starts_with("pip install")
        || lower.starts_with("pip3 install")
        || lower.starts_with("python -m pip")
    {
        return Some(BuildCommand::Build);
    }

    // Lint commands
    if lower.starts_with("cargo clippy")
        || lower.starts_with("cargo fmt")
        || lower.starts_with("npx eslint")
        || lower.starts_with("npm run lint")
        || lower.starts_with("dotnet format")
        || lower.starts_with("pylint")
        || lower.starts_with("flake8")
        || lower.starts_with("ruff check")
        || lower.starts_with("golint")
        || lower.starts_with("go vet")
    {
        return Some(BuildCommand::Lint);
    }

    None
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

pub struct BuildOptimizer {
    test_max_failure_lines: usize,
    test_max_error_lines: usize,
    test_max_warnings: usize,
    build_max_error_lines: usize,
    build_max_warnings: usize,
    lint_max_issue_lines: usize,
}

impl Default for BuildOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOptimizer {
    pub fn new() -> Self {
        Self::from_config(&BuildOptimizerConfig::default())
    }

    /// Create a `BuildOptimizer` from the configuration.
    pub fn from_config(cfg: &BuildOptimizerConfig) -> Self {
        Self {
            test_max_failure_lines: cfg.test_max_failure_lines,
            test_max_error_lines: cfg.test_max_error_lines,
            test_max_warnings: cfg.test_max_warnings,
            build_max_error_lines: cfg.build_max_error_lines,
            build_max_warnings: cfg.build_max_warnings,
            lint_max_issue_lines: cfg.lint_max_issue_lines,
        }
    }
}

impl Optimizer for BuildOptimizer {
    fn name(&self) -> &'static str {
        "build"
    }

    fn can_handle(&self, ctx: &CommandContext) -> bool {
        let lower = ctx.core.to_ascii_lowercase();
        classify(&lower).is_some()
    }

    fn optimize_output(&self, ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput> {
        let lower = ctx.core.to_ascii_lowercase();
        let cmd = classify(&lower).unwrap_or(BuildCommand::Build);

        let optimized = match cmd {
            BuildCommand::Test => compact_test_output(
                raw_output,
                self.test_max_failure_lines,
                self.test_max_error_lines,
                self.test_max_warnings,
            ),
            BuildCommand::Build => compact_build_output(
                raw_output,
                self.build_max_error_lines,
                self.build_max_warnings,
            ),
            BuildCommand::Lint => compact_lint_output(raw_output, self.lint_max_issue_lines),
        };

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Test output — show failures, collapse successes
// ---------------------------------------------------------------------------

/// Compact test output: show failures in full, collapse passing tests.
///
/// Detects common test output formats:
/// - Rust: `test name ... ok` / `test name ... FAILED`
/// - Jest/Vitest: `✓ name` / `✕ name` / `PASS` / `FAIL`
/// - pytest: `PASSED` / `FAILED` / `ERROR`
/// - dotnet: `Passed!` / `Failed!`
/// - Go: `--- PASS:` / `--- FAIL:`
fn compact_test_output(
    raw_output: &str,
    max_failure_lines: usize,
    max_error_lines: usize,
    max_warnings: usize,
) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No test output".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let total_lines = lines.len();

    // Categorize lines
    let mut failures: Vec<&str> = Vec::new();
    let mut errors: Vec<&str> = Vec::new();
    let mut warnings: Vec<&str> = Vec::new();
    let mut summary_lines: Vec<&str> = Vec::new();
    let mut pass_count = 0usize;
    let mut compiling_count = 0usize;
    let mut in_failure_block = false;

    for line in &lines {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();

        // Detect compile/download lines (noise)
        if lower.starts_with("compiling ")
            || lower.starts_with("downloading ")
            || lower.starts_with("downloaded ")
            || lower.starts_with("fresh ")
            || lower.starts_with("installing ")
            || lower.starts_with("resolving ")
            || lower.starts_with("updating ")
        {
            compiling_count += 1;
            continue;
        }

        // Detect summary lines (keep these)
        if is_test_summary_line(l) {
            summary_lines.push(l);
            in_failure_block = false;
            continue;
        }

        // Detect failure/error lines
        if is_failure_line(l) {
            in_failure_block = true;
            failures.push(l);
            continue;
        }

        // Detect error markers
        if is_error_line(l) {
            errors.push(l);
            in_failure_block = true;
            continue;
        }

        // Detect warning lines
        if is_warning_line(l) {
            warnings.push(l);
            continue;
        }

        // If in a failure block, capture context lines
        if in_failure_block {
            if l.is_empty() {
                in_failure_block = false;
            } else {
                failures.push(l);
            }
            continue;
        }

        // Detect passing test lines
        if is_pass_line(l) {
            pass_count += 1;
            continue;
        }
    }

    // Build compact output
    let mut result = Vec::new();

    // Compilation notice
    if compiling_count > 0 {
        result.push(format!("[{compiling_count} compilation steps]"));
    }

    // Failures (show in full)
    if !failures.is_empty() {
        result.push("FAILURES:".to_string());
        // Limit failure output to avoid massive output
        for (i, line) in failures.iter().enumerate() {
            if i >= max_failure_lines {
                result.push(format!(
                    "...+{} more failure lines",
                    failures.len() - max_failure_lines
                ));
                break;
            }
            result.push(line.to_string());
        }
    }

    // Errors
    if !errors.is_empty() {
        result.push("ERRORS:".to_string());
        for (i, line) in errors.iter().enumerate() {
            if i >= max_error_lines {
                result.push(format!(
                    "...+{} more error lines",
                    errors.len() - max_error_lines
                ));
                break;
            }
            result.push(line.to_string());
        }
    }

    // Warnings (condensed) -- test
    if !warnings.is_empty() {
        if warnings.len() <= max_warnings {
            for w in &warnings {
                result.push(w.to_string());
            }
        } else {
            for w in warnings.iter().take(max_warnings) {
                result.push(w.to_string());
            }
            result.push(format!(
                "...+{} more warnings",
                warnings.len() - max_warnings
            ));
        }
    }

    // Pass summary
    if pass_count > 0 {
        result.push(format!("[{pass_count} tests passed]"));
    }

    // Summary lines (always shown)
    for line in &summary_lines {
        result.push(line.to_string());
    }

    // If we didn't extract anything meaningful, return a truncated version
    if result.is_empty() {
        return truncate_output(trimmed, total_lines, 50);
    }

    result.join("\n")
}

/// Check if a line is a test result summary.
fn is_test_summary_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    // Rust test summary
    l.starts_with("test result:")
        // Jest/Vitest summary
        || l.starts_with("test suites:")
        || l.starts_with("tests:")
        || l.starts_with("time:")
        // pytest summary
        || (l.contains("passed") && (l.contains("failed") || l.contains("error") || l.contains("warning")))
        && (l.starts_with('=') || l.contains(" in "))
        // dotnet test summary
        || l.starts_with("passed!")
        || l.starts_with("failed!")
        || l.starts_with("total tests:")
        // Go summary
        || l.starts_with("ok  \t")
        || l.starts_with("fail\t")
        // Generic pass/fail summary
        || (l.contains("passed") && l.contains("failed") && l.len() < 100)
        // Maven/Gradle summary
        || l.starts_with("build success")
        || l.starts_with("build failure")
        || l.starts_with("tests run:")
}

/// Check if a line indicates a test failure.
fn is_failure_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    // Rust: test name ... FAILED
    (l.contains("... failed") || l.contains("...failed"))
        // Jest/Vitest: ✕ or FAIL
        || l.starts_with("✕") || l.starts_with("×")
        || (l.starts_with("fail") && !l.starts_with("fail\t"))
        // pytest: FAILED
        || l.contains("failed")
            && (l.starts_with("failed ") || l.starts_with("f ") || l.contains("::"))
        // Go: --- FAIL:
        || l.starts_with("--- fail:")
        // assertion/panic
        || l.contains("assertion") && (l.contains("failed") || l.contains("error"))
        || l.starts_with("thread '") && l.contains("panicked")
}

/// Check if a line indicates an error.
fn is_error_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("error")
        || l.starts_with("e ")
        || l.contains("error:")
        || l.contains("error[")
        || l.starts_with("fatal:")
}

/// Check if a line indicates a warning.
fn is_warning_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("warning")
        || l.starts_with("warn ")
        || l.contains("warning:")
        || l.contains("warning[")
}

/// Check if a line indicates a passing test.
fn is_pass_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    // Rust: test name ... ok
    (l.contains("... ok") || l.contains("...ok"))
        // Jest/Vitest: ✓ or PASS
        || l.starts_with("✓")
        || l.starts_with("✔")
        || (l.starts_with("pass") && !l.starts_with("passed"))
        // pytest: PASSED or .
        || l.ends_with("passed")
        // Go: --- PASS:
        || l.starts_with("--- pass:")
        // dotnet
        || l.starts_with("  passed ")
        || l.starts_with("  ✓ ")
}

// ---------------------------------------------------------------------------
// Build output — success/fail + error details
// ---------------------------------------------------------------------------

/// Compact build/install output: show success/fail + errors only.
fn compact_build_output(raw_output: &str, max_error_lines: usize, max_warnings: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "Build completed (no output)".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();

    let mut errors: Vec<&str> = Vec::new();
    let mut warnings: Vec<&str> = Vec::new();
    let mut summary_lines: Vec<&str> = Vec::new();
    let mut noise_count = 0usize;
    let mut in_error_block = false;

    for line in &lines {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();

        // Skip compile/download noise
        if lower.starts_with("compiling ")
            || lower.starts_with("downloading ")
            || lower.starts_with("downloaded ")
            || lower.starts_with("fresh ")
            || lower.starts_with("installing ")
            || lower.starts_with("resolving ")
            || lower.starts_with("updating ")
            || lower.starts_with("added ")
            || lower.starts_with("removed ")
            || lower.starts_with("changed ")
            || lower.starts_with("packages ")
            || lower.starts_with("npm warn")
            || lower.starts_with("up to date")
            || lower.starts_with("audited ")
            || lower.starts_with("found 0 ")
            || lower.starts_with("restore complete")
            || lower.starts_with("  determining projects")
            || lower.starts_with("  restored ")
        {
            noise_count += 1;
            continue;
        }

        // Summary/success lines
        if lower.starts_with("finished")
            || lower.starts_with("build succeeded")
            || lower.starts_with("build success")
            || lower.contains("compiled successfully")
            || lower.starts_with("successfully ")
        {
            summary_lines.push(l);
            in_error_block = false;
            continue;
        }

        // Error lines
        if is_error_line(l) {
            errors.push(l);
            in_error_block = true;
            continue;
        }

        // Error context
        if in_error_block {
            if l.is_empty() {
                in_error_block = false;
            } else {
                errors.push(l);
            }
            continue;
        }

        // Warning lines
        if is_warning_line(l) {
            warnings.push(l);
            continue;
        }
    }

    let mut result = Vec::new();

    // Indicate noise was removed
    if noise_count > 0 {
        result.push(format!("[{noise_count} build steps]"));
    }

    // Errors (show in full, with limit)
    if !errors.is_empty() {
        result.push("ERRORS:".to_string());
        for (i, line) in errors.iter().enumerate() {
            if i >= max_error_lines {
                result.push(format!(
                    "...+{} more error lines",
                    errors.len() - max_error_lines
                ));
                break;
            }
            result.push(line.to_string());
        }
    }

    // Warnings (condensed) -- build
    if !warnings.is_empty() {
        if warnings.len() <= max_warnings {
            for w in &warnings {
                result.push(w.to_string());
            }
        } else {
            for w in warnings.iter().take(max_warnings) {
                result.push(w.to_string());
            }
            result.push(format!(
                "...+{} more warnings",
                warnings.len() - max_warnings
            ));
        }
    }

    // Summary lines
    for line in &summary_lines {
        result.push(line.to_string());
    }

    // If nothing meaningful extracted, infer success/failure
    if result.is_empty() {
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("error") || lower.contains("failed") || lower.contains("fatal") {
            return truncate_output(trimmed, lines.len(), 40);
        }
        return "Build succeeded".to_string();
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// Lint output — warnings and errors only
// ---------------------------------------------------------------------------

/// Compact lint output: keep errors and warnings, discard noise.
fn compact_lint_output(raw_output: &str, max_issue_lines: usize) -> String {
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return "No lint issues found".to_string();
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let mut issues: Vec<&str> = Vec::new();
    let mut summary_lines: Vec<&str> = Vec::new();
    let mut in_issue_block = false;

    for line in &lines {
        let l = line.trim();
        let lower = l.to_ascii_lowercase();

        // Skip noise
        if lower.starts_with("checking ")
            || lower.starts_with("compiling ")
            || lower.starts_with("finished")
        {
            continue;
        }

        // Summary lines
        if lower.starts_with("warning:") && lower.contains("generated")
            || lower.starts_with("error: could not compile")
            || lower.contains("problems found")
            || lower.contains("errors and")
            || lower.contains("0 errors")
        {
            summary_lines.push(l);
            in_issue_block = false;
            continue;
        }

        // Issue lines (errors and warnings)
        if is_error_line(l) || is_warning_line(l) {
            issues.push(l);
            in_issue_block = true;
            continue;
        }

        // Issue context (indented lines after an issue)
        if in_issue_block {
            if l.is_empty() {
                in_issue_block = false;
            } else {
                issues.push(l);
            }
            continue;
        }
    }

    let mut result = Vec::new();

    // Issues
    if !issues.is_empty() {
        for (i, line) in issues.iter().enumerate() {
            if i >= max_issue_lines {
                result.push(format!(
                    "...+{} more issue lines",
                    issues.len() - max_issue_lines
                ));
                break;
            }
            result.push(line.to_string());
        }
    }

    // Summary
    for line in &summary_lines {
        result.push(line.to_string());
    }

    if result.is_empty() {
        // No issues detected — clean lint
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("error") || lower.contains("warning") {
            return truncate_output(trimmed, lines.len(), 40);
        }
        return "No lint issues found".to_string();
    }

    result.join("\n")
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Generic truncation for output that doesn't match specific patterns.
fn truncate_output(text: &str, total_lines: usize, max_lines: usize) -> String {
    if total_lines <= max_lines {
        return text.to_string();
    }

    let head: Vec<&str> = text.lines().take(max_lines).collect();
    let mut result = head.join("\n");
    result.push_str(&format!(
        "\n...({} lines omitted, {} total)",
        total_lines - max_lines,
        total_lines
    ));
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::CommandContext;

    // classify -----------------------------------------------------------

    #[test]
    fn classifies_test_commands() {
        assert_eq!(classify("cargo test"), Some(BuildCommand::Test));
        assert_eq!(classify("cargo test --lib"), Some(BuildCommand::Test));
        assert_eq!(classify("npm test"), Some(BuildCommand::Test));
        assert_eq!(classify("npm run test"), Some(BuildCommand::Test));
        assert_eq!(classify("npx jest"), Some(BuildCommand::Test));
        assert_eq!(classify("dotnet test"), Some(BuildCommand::Test));
        assert_eq!(classify("pytest"), Some(BuildCommand::Test));
        assert_eq!(classify("python -m pytest"), Some(BuildCommand::Test));
        assert_eq!(classify("go test ./..."), Some(BuildCommand::Test));
        assert_eq!(classify("make test"), Some(BuildCommand::Test));
    }

    #[test]
    fn classifies_build_commands() {
        assert_eq!(classify("cargo build"), Some(BuildCommand::Build));
        assert_eq!(classify("cargo build --release"), Some(BuildCommand::Build));
        assert_eq!(classify("npm install"), Some(BuildCommand::Build));
        assert_eq!(classify("npm ci"), Some(BuildCommand::Build));
        assert_eq!(classify("npm run build"), Some(BuildCommand::Build));
        assert_eq!(classify("dotnet build"), Some(BuildCommand::Build));
        assert_eq!(classify("go build ./cmd/server"), Some(BuildCommand::Build));
        assert_eq!(classify("make"), Some(BuildCommand::Build));
        assert_eq!(
            classify("pip install -r requirements.txt"),
            Some(BuildCommand::Build)
        );
    }

    #[test]
    fn classifies_lint_commands() {
        assert_eq!(classify("cargo clippy"), Some(BuildCommand::Lint));
        assert_eq!(classify("cargo fmt --check"), Some(BuildCommand::Lint));
        assert_eq!(classify("npx eslint ."), Some(BuildCommand::Lint));
        assert_eq!(classify("npm run lint"), Some(BuildCommand::Lint));
        assert_eq!(classify("ruff check ."), Some(BuildCommand::Lint));
    }

    #[test]
    fn does_not_classify_unrelated() {
        assert_eq!(classify("git status"), None);
        assert_eq!(classify("ls -la"), None);
        assert_eq!(classify("docker ps"), None);
    }

    // can_handle ---------------------------------------------------------

    #[test]
    fn handles_build_with_prefix() {
        let opt = BuildOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("cd /repo && cargo test")));
        assert!(opt.can_handle(&CommandContext::new("RUST_LOG=debug cargo build")));
        assert!(opt.can_handle(&CommandContext::new("npm test")));
    }

    // compact_test_output ------------------------------------------------

    #[test]
    fn test_output_empty() {
        assert_eq!(compact_test_output("", 80, 40, 10), "No test output");
    }

    #[test]
    fn test_output_rust_all_pass() {
        let input = "\
   Compiling terse v0.1.0
   Compiling serde v1.0.200
running 5 tests
test utils::tests::test_one ... ok
test utils::tests::test_two ... ok
test utils::tests::test_three ... ok
test utils::tests::test_four ... ok
test utils::tests::test_five ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";

        let result = compact_test_output(input, 80, 40, 10);
        assert!(result.contains("[2 compilation steps]"));
        assert!(result.contains("[5 tests passed]"));
        assert!(result.contains("test result:"));
        // No individual test lines
        assert!(!result.contains("test utils::tests::test_one"));
    }

    #[test]
    fn test_output_rust_with_failure() {
        let input = "\
running 3 tests
test good_test ... ok
test bad_test ... FAILED
test another_good ... ok

failures:
---- bad_test stdout ----
thread 'bad_test' panicked at 'assertion failed'

test result: FAILED. 2 passed; 1 failed; 0 ignored";

        let result = compact_test_output(input, 80, 40, 10);
        assert!(result.contains("FAILURES:"));
        assert!(result.contains("bad_test"));
        assert!(result.contains("panicked"));
        assert!(result.contains("test result:"));
    }

    // compact_build_output -----------------------------------------------

    #[test]
    fn build_output_empty() {
        assert_eq!(
            compact_build_output("", 60, 10),
            "Build completed (no output)"
        );
    }

    #[test]
    fn build_output_cargo_success() {
        let input = "\
   Compiling serde v1.0.200
   Compiling anyhow v1.0.86
   Compiling terse v0.1.0
    Finished dev [unoptimized + debuginfo] target(s) in 5.32s";

        let result = compact_build_output(input, 60, 10);
        assert!(result.contains("[3 build steps]"));
        assert!(result.contains("Finished"));
    }

    #[test]
    fn build_output_npm_success() {
        let input = "\
added 150 packages, and audited 151 packages in 3s
found 0 vulnerabilities";

        let result = compact_build_output(input, 60, 10);
        assert!(!result.contains("added 150"));
    }

    #[test]
    fn build_output_with_errors() {
        let input = "\
   Compiling terse v0.1.0
error[E0308]: mismatched types
 --> src/main.rs:10:5
  |
10 |     42
  |     ^^ expected `String`, found integer

error: could not compile `terse`";

        let result = compact_build_output(input, 60, 10);
        assert!(result.contains("ERRORS:"));
        assert!(result.contains("mismatched types"));
    }

    // compact_lint_output ------------------------------------------------

    #[test]
    fn lint_output_clean() {
        let input = "\
    Checking terse v0.1.0
    Finished dev [unoptimized + debuginfo] target(s)";

        let result = compact_lint_output(input, 80);
        assert_eq!(result, "No lint issues found");
    }

    #[test]
    fn lint_output_with_warnings() {
        let input = "\
    Checking terse v0.1.0
warning: unused variable: `x`
 --> src/main.rs:5:9
  |
5 |     let x = 42;
  |         ^ help: if this is intentional, prefix it with an underscore: `_x`

warning: `terse` (bin \"terse\") generated 1 warning";

        let result = compact_lint_output(input, 80);
        assert!(result.contains("unused variable"));
        assert!(result.contains("generated 1 warning"));
    }
}
