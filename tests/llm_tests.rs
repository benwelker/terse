/// Integration tests for the LLM Smart Path (Phase 3).
///
/// Unit tests for individual LLM submodules live in each file's `#[cfg(test)]`
/// block. These tests exercise cross-module behavior:
///
/// - Feature-flag gating
/// - Prompt construction end-to-end
/// - Validation edge cases across the pipeline
///
/// Tests that require a live Ollama instance are gated behind the
/// `TERSE_TEST_LLM` environment variable (set to `1` to run).
///
/// # Safety
///
/// Several tests use `std::env::set_var` / `remove_var` which are `unsafe` in
/// Rust 2024 edition. These tests are inherently single-threaded (Rust test
/// runner executes `#[test]` sequentially within a binary unless `--test-threads`
/// is set otherwise). The `unsafe` blocks are sound because no other thread
/// reads these variables concurrently.
use terse::llm::config::SmartPathConfig;
use terse::llm::prompts::{build_prompt, classify_command, CommandCategory};
use terse::llm::validation::validate_llm_output;

/// Helper: set an env var (wraps the `unsafe` call).
///
/// # Safety
/// Must only be called from single-threaded test contexts.
unsafe fn set_env(key: &str, val: &str) {
    unsafe { std::env::set_var(key, val) }
}

/// Helper: remove an env var (wraps the `unsafe` call).
///
/// # Safety
/// Must only be called from single-threaded test contexts.
unsafe fn remove_env(key: &str) {
    unsafe { std::env::remove_var(key) }
}

// ---------------------------------------------------------------------------
// Feature flag config tests
//
// These tests mutate process-wide environment variables, so they are combined
// into a single #[test] to avoid racing with each other when Cargo runs tests
// in parallel.
// ---------------------------------------------------------------------------

#[test]
fn smart_path_feature_flag_env_vars() {
    // --- disabled by default ---
    unsafe { remove_env("TERSE_SMART_PATH") };
    let config = SmartPathConfig::load();
    assert!(!config.enabled, "smart path should be disabled by default");

    // --- "1" enables ---
    unsafe { set_env("TERSE_SMART_PATH", "1") };
    let config = SmartPathConfig::load();
    assert!(config.enabled, "TERSE_SMART_PATH=1 should enable");
    unsafe { remove_env("TERSE_SMART_PATH") };

    // --- "true" enables ---
    unsafe { set_env("TERSE_SMART_PATH", "true") };
    let config = SmartPathConfig::load();
    assert!(config.enabled, "TERSE_SMART_PATH=true should enable");
    unsafe { remove_env("TERSE_SMART_PATH") };

    // --- "false" disables ---
    unsafe { set_env("TERSE_SMART_PATH", "false") };
    let config = SmartPathConfig::load();
    assert!(!config.enabled, "TERSE_SMART_PATH=false should disable");
    unsafe { remove_env("TERSE_SMART_PATH") };

    // --- "0" disables ---
    unsafe { set_env("TERSE_SMART_PATH", "0") };
    let config = SmartPathConfig::load();
    assert!(!config.enabled, "TERSE_SMART_PATH=0 should disable");
    unsafe { remove_env("TERSE_SMART_PATH") };

    // --- model override ---
    unsafe { set_env("TERSE_SMART_PATH_MODEL", "phi3:mini") };
    let config = SmartPathConfig::load();
    assert_eq!(config.model, "phi3:mini");
    unsafe { remove_env("TERSE_SMART_PATH_MODEL") };

    // --- url override ---
    unsafe { set_env("TERSE_SMART_PATH_URL", "http://myhost:9999") };
    let config = SmartPathConfig::load();
    assert_eq!(config.ollama_url, "http://myhost:9999");
    unsafe { remove_env("TERSE_SMART_PATH_URL") };

    // --- timeout override ---
    unsafe { set_env("TERSE_SMART_PATH_TIMEOUT_MS", "3000") };
    let config = SmartPathConfig::load();
    assert_eq!(config.timeout_ms, 3000);
    unsafe { remove_env("TERSE_SMART_PATH_TIMEOUT_MS") };

    // --- min-chars override ---
    unsafe { set_env("TERSE_SMART_PATH_MIN_CHARS", "500") };
    let config = SmartPathConfig::load();
    assert_eq!(config.min_output_chars, 500);
    unsafe { remove_env("TERSE_SMART_PATH_MIN_CHARS") };
}

// ---------------------------------------------------------------------------
// Category classification — cross-module verification
// ---------------------------------------------------------------------------

#[test]
fn category_selection_for_various_commands() {
    let cases = vec![
        ("git diff --cached", CommandCategory::VersionControl),
        ("svn status", CommandCategory::VersionControl),
        ("ls -la /tmp", CommandCategory::FileOperations),
        ("find . -name '*.rs'", CommandCategory::FileOperations),
        ("cargo test --release", CommandCategory::BuildTest),
        ("npm run build", CommandCategory::BuildTest),
        ("docker compose up", CommandCategory::ContainerTools),
        ("kubectl get pods -n default", CommandCategory::ContainerTools),
        ("journalctl -u nginx --since today", CommandCategory::Logs),
        ("dmesg --level=err", CommandCategory::Logs),
        ("whoami", CommandCategory::Generic),
        ("curl -s http://example.com", CommandCategory::Generic),
    ];

    for (cmd, expected) in cases {
        let actual = classify_command(cmd);
        assert_eq!(actual, expected, "classify_command({cmd:?})");
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

#[test]
fn prompt_contains_all_sections() {
    let prompt = build_prompt("docker ps", "CONTAINER ID   IMAGE   STATUS\nabc123  nginx  Up 2h");

    assert!(prompt.contains("## Rules"), "prompt should have Rules section");
    assert!(prompt.contains("## Example"), "prompt should have Example section");
    assert!(prompt.contains("## Command"), "prompt should have Command section");
    assert!(prompt.contains("`docker ps`"), "prompt should contain the command");
    assert!(prompt.contains("## Raw output"), "prompt should have Raw output");
    assert!(prompt.contains("abc123"), "prompt should contain raw output content");
    assert!(prompt.contains("## Condensed output"), "prompt should end with output marker");
}

#[test]
fn prompt_uses_category_specific_rules() {
    let git_prompt = build_prompt("git log", "commit abc123\nAuthor: test");
    assert!(git_prompt.contains("branch"), "git prompt should mention branch");

    let docker_prompt = build_prompt("docker ps", "CONTAINER ID");
    assert!(docker_prompt.contains("container"), "docker prompt should mention containers");

    let build_prompt_text = build_prompt("cargo test", "running 5 tests\ntest ok");
    assert!(build_prompt_text.contains("error"), "build prompt should mention errors");
}

// ---------------------------------------------------------------------------
// Validation — integration scenarios
// ---------------------------------------------------------------------------

#[test]
fn validation_accepts_good_condensation() {
    let raw = "This is a verbose output with lots of unnecessary detail that goes on and on.";
    let condensed = "Verbose output summary.";
    assert!(validate_llm_output(raw, condensed).is_ok());
}

#[test]
fn validation_rejects_expansion() {
    let raw = "Short.";
    let expanded = "This is a much longer expansion that the LLM produced instead of condensing the original output which was just the word Short.";
    assert!(validate_llm_output(raw, expanded).is_err());
}

#[test]
fn validation_rejects_chatbot_preamble() {
    let raw = "Some verbose output that needs condensing and is long enough to pass the length check.";
    let with_preamble = "Here is the condensed version.";
    assert!(validate_llm_output(raw, with_preamble).is_err());
}

#[test]
fn validation_rejects_apology() {
    let raw = "Error: connection refused at localhost:5432 with extended details about the failure.";
    let apology = "I apologize, connection refused.";
    assert!(validate_llm_output(raw, apology).is_err());
}

// ---------------------------------------------------------------------------
// Live Ollama tests (gated behind TERSE_TEST_LLM=1)
// ---------------------------------------------------------------------------

/// Test that the full LLM pipeline works with a real Ollama instance.
///
/// To run: `TERSE_TEST_LLM=1 cargo test llm_live`
///
/// Requires Ollama running locally with `llama3.2:1b` (or the configured model).
#[test]
fn llm_live_generate_and_validate() {
    if std::env::var("TERSE_TEST_LLM").unwrap_or_default() != "1" {
        eprintln!("Skipping live LLM test (set TERSE_TEST_LLM=1 to enable)");
        return;
    }

    use terse::llm::ollama::OllamaClient;

    let config = SmartPathConfig {
        enabled: true,
        ..SmartPathConfig::default()
    };

    let client = OllamaClient::from_config(&config);

    // Health check
    assert!(client.is_healthy(), "Ollama should be reachable and have models");

    // Generate
    let prompt = build_prompt(
        "git status",
        "On branch main\nYour branch is up to date with 'origin/main'.\n\nnothing to commit, working tree clean\n",
    );
    let result = client.generate(&prompt);
    assert!(result.is_ok(), "generate should succeed: {:?}", result.err());

    let output = result.unwrap();
    assert!(!output.trim().is_empty(), "LLM output should be non-empty");
}

/// Test the full `optimize_with_llm` pipeline end-to-end.
#[test]
fn llm_live_optimize_end_to_end() {
    if std::env::var("TERSE_TEST_LLM").unwrap_or_default() != "1" {
        eprintln!("Skipping live LLM E2E test (set TERSE_TEST_LLM=1 to enable)");
        return;
    }

    // Enable the smart path for this test
    unsafe { set_env("TERSE_SMART_PATH", "1") };

    let raw_output = "\
On branch main\n\
Your branch is ahead of 'origin/main' by 3 commits.\n\
  (use \"git push\" to publish your local commits)\n\
\n\
Changes not staged for commit:\n\
  (use \"git add <file>...\" to update what will be committed)\n\
  (use \"git restore <file>...\" to discard changes in working directory)\n\
        modified:   src/main.rs\n\
        modified:   src/lib.rs\n\
        modified:   Cargo.toml\n\
\n\
Untracked files:\n\
  (use \"git add <file>...\" to include in what will be committed)\n\
        src/llm/\n\
\n\
no changes added to commit (use \"git add\" and/or \"git commit -a\")\n";

    let result = terse::llm::optimize_with_llm("git status", raw_output);

    unsafe { remove_env("TERSE_SMART_PATH") };

    assert!(result.is_ok(), "optimize_with_llm should succeed: {:?}", result.err());
    let llm_result = result.unwrap();
    assert!(
        llm_result.output.len() < raw_output.len(),
        "LLM output ({}) should be shorter than raw ({})",
        llm_result.output.len(),
        raw_output.len()
    );
    assert!(llm_result.optimized_tokens < llm_result.original_tokens);
    assert!(llm_result.latency_ms > 0);
}
