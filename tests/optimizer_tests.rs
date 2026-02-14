use terse::matching::{extract_core_command, is_terse_invocation};
use terse::optimizers::{
    BuildOptimizer, CommandContext, DockerOptimizer, FileOptimizer, GenericOptimizer, GitOptimizer,
    Optimizer,
};

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — basic commands
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_handles_git_commands() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("git status")));
    assert!(optimizer.can_handle(&CommandContext::new("cd /repo && git diff")));
    assert!(!optimizer.can_handle(&CommandContext::new("cargo test")));
}

#[test]
fn git_optimizer_handles_short_status_commands() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("git push")));
    assert!(optimizer.can_handle(&CommandContext::new("git pull")));
    assert!(optimizer.can_handle(&CommandContext::new("git fetch")));
    assert!(optimizer.can_handle(&CommandContext::new("git add .")));
    assert!(optimizer.can_handle(&CommandContext::new("git commit -m \"test\"")));
}

#[test]
fn git_optimizer_rejects_non_git_commands() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle(&CommandContext::new("cargo test")));
    assert!(!optimizer.can_handle(&CommandContext::new("npm install")));
    assert!(!optimizer.can_handle(&CommandContext::new("echo 'git status'")));
}

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — wrapped / prefixed commands via matching engine
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_handles_env_var_prefix() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("LANG=C git diff")));
    assert!(optimizer.can_handle(&CommandContext::new("GIT_PAGER=cat git log")));
    assert!(optimizer.can_handle(&CommandContext::new("LANG=C GIT_DIR=/foo git status")));
}

#[test]
fn git_optimizer_handles_subshell() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("(cd /repo && git status)")));
    assert!(optimizer.can_handle(&CommandContext::new("(git diff)")));
}

#[test]
fn git_optimizer_handles_shell_wrapper() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("bash -c \"git status\"")));
    assert!(optimizer.can_handle(&CommandContext::new("sh -c 'git diff'")));
}

#[test]
fn git_optimizer_handles_pipeline() {
    let optimizer = GitOptimizer::new();

    // The core command before the pipe is "git log" which is optimizable
    assert!(optimizer.can_handle(&CommandContext::new("git log | head -20")));
}

#[test]
fn git_optimizer_handles_complex_cd_env_combo() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("cd /home/user && LANG=C git status")));
    assert!(optimizer.can_handle(&CommandContext::new(
        "cd /project && GIT_DIR=/foo git diff --cached"
    )));
}

#[test]
fn git_optimizer_preserves_args_through_matching() {
    let optimizer = GitOptimizer::new();

    // Ensure --format=oneline is NOT stripped as an env var
    assert!(optimizer.can_handle(&CommandContext::new("cd /repo && git log --format=oneline")));
}

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — new subcommands (show, stash, worktree)
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_handles_show_stash_worktree() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle(&CommandContext::new("git show abc1234")));
    assert!(optimizer.can_handle(&CommandContext::new("git show")));
    assert!(optimizer.can_handle(&CommandContext::new("git stash list")));
    assert!(optimizer.can_handle(&CommandContext::new("git stash show")));
    assert!(optimizer.can_handle(&CommandContext::new("git stash")));
    assert!(optimizer.can_handle(&CommandContext::new("git worktree list")));
    assert!(optimizer.can_handle(&CommandContext::new("git worktree")));
}

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — action-mode passthrough
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_branch_action_passthrough() {
    let optimizer = GitOptimizer::new();

    // Destructive / rename / copy operations pass through
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -d feature")));
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -D feature")));
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -m old new")));
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -M old new")));
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -c old new")));
    assert!(!optimizer.can_handle(&CommandContext::new("git branch -C old new")));

    // List operations are still handled
    assert!(optimizer.can_handle(&CommandContext::new("git branch")));
    assert!(optimizer.can_handle(&CommandContext::new("git branch -a")));
    assert!(optimizer.can_handle(&CommandContext::new("git branch -r")));
}

#[test]
fn git_optimizer_worktree_action_passthrough() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle(&CommandContext::new("git worktree add ../feat feature")));
    assert!(!optimizer.can_handle(&CommandContext::new("git worktree remove ../feat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git worktree prune")));
    assert!(!optimizer.can_handle(&CommandContext::new("git worktree lock ../feat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git worktree unlock ../feat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git worktree move ../feat ../new")));
}

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — already-compact flag passthrough
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_skips_compact_status() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle(&CommandContext::new("git status --short")));
    assert!(!optimizer.can_handle(&CommandContext::new("git status -s")));
    assert!(!optimizer.can_handle(&CommandContext::new("git status --porcelain")));
    assert!(!optimizer.can_handle(&CommandContext::new("git status -v")));
    assert!(!optimizer.can_handle(&CommandContext::new("git status --verbose")));

    // Bare status IS still handled
    assert!(optimizer.can_handle(&CommandContext::new("git status")));
    assert!(optimizer.can_handle(&CommandContext::new("git status --branch")));
}

#[test]
fn git_optimizer_skips_stat_diff() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle(&CommandContext::new("git diff --stat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git diff --numstat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git diff --shortstat")));

    // Bare diff and other flag combinations ARE handled
    assert!(optimizer.can_handle(&CommandContext::new("git diff")));
    assert!(optimizer.can_handle(&CommandContext::new("git diff --cached")));
    assert!(optimizer.can_handle(&CommandContext::new("git diff HEAD~1")));
}

#[test]
fn git_optimizer_skips_formatted_show() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle(&CommandContext::new("git show --stat")));
    assert!(!optimizer.can_handle(&CommandContext::new("git show --pretty=format:%H")));
    assert!(!optimizer.can_handle(&CommandContext::new("git show --format=oneline")));

    // Bare show IS handled
    assert!(optimizer.can_handle(&CommandContext::new("git show")));
    assert!(optimizer.can_handle(&CommandContext::new("git show abc1234")));
}

// ---------------------------------------------------------------------------
// Matching engine integration: extract_core_command
// ---------------------------------------------------------------------------

#[test]
fn matching_extracts_core_from_cd_chain() {
    assert_eq!(extract_core_command("cd /repo && git status"), "git status");
}

#[test]
fn matching_extracts_core_from_bash_wrapper() {
    assert_eq!(
        extract_core_command("bash -c \"cd /repo && git status\""),
        "git status"
    );
}

// ---------------------------------------------------------------------------
// Terse invocation loop guard
// ---------------------------------------------------------------------------

#[test]
fn terse_loop_guard_detects_invocation() {
    assert!(is_terse_invocation("terse run \"git status\""));
    assert!(is_terse_invocation("terse.exe run \"git status\""));
    assert!(is_terse_invocation(
        "\"C:\\source\\terse\\target\\terse.exe\" run \"git status\""
    ));
}

#[test]
fn terse_loop_guard_rejects_non_terse() {
    assert!(!is_terse_invocation("git status"));
    assert!(!is_terse_invocation(
        "cd /my-terse-run-project && git status"
    ));
    assert!(!is_terse_invocation("terse stats"));
}

// ===========================================================================
// FileOptimizer — integration tests
// ===========================================================================

#[test]
fn file_optimizer_handles_ls() {
    let opt = FileOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("ls -la")));
    assert!(opt.can_handle(&CommandContext::new("ls")));
    assert!(opt.can_handle(&CommandContext::new("cd /repo && ls -la")));
    assert!(opt.can_handle(&CommandContext::new("dir")));
}

#[test]
fn file_optimizer_skips_compact_ls() {
    let opt = FileOptimizer::new();
    assert!(!opt.can_handle(&CommandContext::new("ls -1")));
}

#[test]
fn file_optimizer_handles_find_cat_wc_tree() {
    let opt = FileOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("find . -name '*.rs'")));
    assert!(opt.can_handle(&CommandContext::new("cat README.md")));
    assert!(opt.can_handle(&CommandContext::new("head -n 20 file.txt")));
    assert!(opt.can_handle(&CommandContext::new("tail -100 app.log")));
    assert!(opt.can_handle(&CommandContext::new("wc -l *.txt")));
    assert!(opt.can_handle(&CommandContext::new("tree src/")));
}

#[test]
fn file_optimizer_rejects_unrelated() {
    let opt = FileOptimizer::new();
    assert!(!opt.can_handle(&CommandContext::new("git status")));
    assert!(!opt.can_handle(&CommandContext::new("cargo build")));
    assert!(!opt.can_handle(&CommandContext::new("docker ps")));
}

#[test]
fn file_optimizer_ls_compacts_long_output() {
    let opt = FileOptimizer::new();
    let ctx = CommandContext::new("ls -la");
    let lines: Vec<String> = (0..100).map(|i| format!("file{i}.txt")).collect();
    let input = lines.join("\n");
    let result = opt.optimize_output(&ctx, &input).unwrap();
    assert!(result.output.contains("+40 more (100 total)"));
    assert!(result.optimized_tokens < input.len() / 4);
}

#[test]
fn file_optimizer_find_compacts_many_results() {
    let opt = FileOptimizer::new();
    let ctx = CommandContext::new("find . -name '*.rs'");
    let lines: Vec<String> = (0..80).map(|i| format!("./src/file{i}.rs")).collect();
    let input = lines.join("\n");
    let result = opt.optimize_output(&ctx, &input).unwrap();
    assert!(result.output.contains("+40 more (80 total)"));
}

#[test]
fn file_optimizer_cat_truncates_long_file() {
    let opt = FileOptimizer::new();
    let ctx = CommandContext::new("cat bigfile.rs");
    let lines: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
    let input = lines.join("\n");
    let result = opt.optimize_output(&ctx, &input).unwrap();
    assert!(result.output.contains("lines omitted"));
    assert!(result.output.contains("line 0"));   // head
    assert!(result.output.contains("line 199")); // tail
}

// ===========================================================================
// BuildOptimizer — integration tests
// ===========================================================================

#[test]
fn build_optimizer_handles_test_commands() {
    let opt = BuildOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("cargo test")));
    assert!(opt.can_handle(&CommandContext::new("npm test")));
    assert!(opt.can_handle(&CommandContext::new("dotnet test")));
    assert!(opt.can_handle(&CommandContext::new("pytest")));
    assert!(opt.can_handle(&CommandContext::new("go test ./...")));
    assert!(opt.can_handle(&CommandContext::new("cd /repo && cargo test")));
    assert!(opt.can_handle(&CommandContext::new("RUST_LOG=debug cargo test")));
}

#[test]
fn build_optimizer_handles_build_commands() {
    let opt = BuildOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("cargo build")));
    assert!(opt.can_handle(&CommandContext::new("cargo build --release")));
    assert!(opt.can_handle(&CommandContext::new("npm install")));
    assert!(opt.can_handle(&CommandContext::new("npm ci")));
    assert!(opt.can_handle(&CommandContext::new("dotnet build")));
    assert!(opt.can_handle(&CommandContext::new("pip install -r requirements.txt")));
    assert!(opt.can_handle(&CommandContext::new("make")));
}

#[test]
fn build_optimizer_handles_lint_commands() {
    let opt = BuildOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("cargo clippy")));
    assert!(opt.can_handle(&CommandContext::new("cargo fmt --check")));
    assert!(opt.can_handle(&CommandContext::new("npx eslint .")));
    assert!(opt.can_handle(&CommandContext::new("ruff check .")));
}

#[test]
fn build_optimizer_rejects_unrelated() {
    let opt = BuildOptimizer::new();
    assert!(!opt.can_handle(&CommandContext::new("git status")));
    assert!(!opt.can_handle(&CommandContext::new("docker ps")));
    assert!(!opt.can_handle(&CommandContext::new("ls -la")));
}

#[test]
fn build_optimizer_compacts_rust_test_output() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("cargo test");
    let input = "\
   Compiling terse v0.1.0
   Compiling serde v1.0.200
running 10 tests
test test_one ... ok
test test_two ... ok
test test_three ... ok
test test_four ... ok
test test_five ... ok
test test_six ... ok
test test_seven ... ok
test test_eight ... ok
test test_nine ... ok
test test_ten ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";

    let result = opt.optimize_output(&ctx, input).unwrap();
    assert!(result.output.contains("[2 compilation steps]"));
    assert!(result.output.contains("[10 tests passed]"));
    assert!(result.output.contains("test result:"));
    // Individual tests should NOT appear
    assert!(!result.output.contains("test test_one"));
}

#[test]
fn build_optimizer_preserves_test_failures() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("cargo test");
    let input = "\
running 3 tests
test good ... ok
test bad ... FAILED
test ok_too ... ok

failures:
---- bad stdout ----
thread 'bad' panicked at 'assertion failed: false'

test result: FAILED. 2 passed; 1 failed; 0 ignored";

    let result = opt.optimize_output(&ctx, input).unwrap();
    assert!(result.output.contains("FAILURES:"));
    assert!(result.output.contains("bad"));
    assert!(result.output.contains("panicked"));
}

#[test]
fn build_optimizer_compacts_build_success() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("cargo build");
    let input = "\
   Compiling serde v1.0.200
   Compiling anyhow v1.0.86
   Compiling terse v0.1.0
    Finished dev [unoptimized + debuginfo] target(s) in 5.32s";

    let result = opt.optimize_output(&ctx, input).unwrap();
    assert!(result.output.contains("[3 build steps]"));
    assert!(result.output.contains("Finished"));
}

// ===========================================================================
// DockerOptimizer — integration tests
// ===========================================================================

#[test]
fn docker_optimizer_handles_commands() {
    let opt = DockerOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("docker ps")));
    assert!(opt.can_handle(&CommandContext::new("docker ps -a")));
    assert!(opt.can_handle(&CommandContext::new("docker images")));
    assert!(opt.can_handle(&CommandContext::new("docker logs myapp")));
    assert!(opt.can_handle(&CommandContext::new("docker compose ps")));
    assert!(opt.can_handle(&CommandContext::new("docker build .")));
    assert!(opt.can_handle(&CommandContext::new("docker pull nginx")));
    assert!(opt.can_handle(&CommandContext::new("docker network ls")));
}

#[test]
fn docker_optimizer_skips_custom_format() {
    let opt = DockerOptimizer::new();
    assert!(!opt.can_handle(&CommandContext::new(
        "docker ps --format '{{.Names}}'"
    )));
    assert!(!opt.can_handle(&CommandContext::new(
        "docker images --format '{{.Repository}}'"
    )));
}

#[test]
fn docker_optimizer_rejects_unrelated() {
    let opt = DockerOptimizer::new();
    assert!(!opt.can_handle(&CommandContext::new("git status")));
    assert!(!opt.can_handle(&CommandContext::new("ls -la")));
    assert!(!opt.can_handle(&CommandContext::new("cargo build")));
}

#[test]
fn docker_optimizer_ps_empty() {
    let opt = DockerOptimizer::new();
    let ctx = CommandContext::new("docker ps");
    let result = opt.optimize_output(&ctx, "").unwrap();
    assert_eq!(result.output, "No containers running");
}

#[test]
fn docker_optimizer_pull_strips_progress() {
    let opt = DockerOptimizer::new();
    let ctx = CommandContext::new("docker pull nginx");
    let input = "\
Using default tag: latest
latest: Pulling from library/nginx
a2abf6c4d29d: Pulling fs layer
a2abf6c4d29d: Downloading
a2abf6c4d29d: Pull complete
Digest: sha256:abc123def456
Status: Downloaded newer image for nginx:latest
docker.io/library/nginx:latest";

    let result = opt.optimize_output(&ctx, input).unwrap();
    assert!(!result.output.contains("Pulling fs layer"));
    assert!(!result.output.contains("Pull complete"));
    assert!(result.output.contains("Digest"));
}

// ===========================================================================
// GenericOptimizer — integration tests
// ===========================================================================

#[test]
fn generic_optimizer_handles_anything() {
    let opt = GenericOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("unknown-tool --verbose")));
    assert!(opt.can_handle(&CommandContext::new("some-custom-script.sh")));
}

#[test]
fn generic_optimizer_collapses_blanks() {
    let opt = GenericOptimizer::new();
    let ctx = CommandContext::new("some-cmd");
    // Build an input > 512 bytes with excessive blank lines
    let mut input = String::new();
    for i in 0..50 {
        input.push_str(&format!("line {i}\n\n\n\n\n"));
    }
    let result = opt.optimize_output(&ctx, &input).unwrap();
    // Should not have 5 consecutive blanks
    assert!(!result.output.contains("\n\n\n\n"));
}

#[test]
fn generic_optimizer_passthrough_small() {
    let opt = GenericOptimizer::new();
    let ctx = CommandContext::new("echo hello");
    let input = "hello";
    let result = opt.optimize_output(&ctx, input).unwrap();
    assert_eq!(result.output, "hello");
}

// ===========================================================================
// OptimizerRegistry — integration tests for multi-optimizer routing
// ===========================================================================

#[test]
fn registry_routes_to_correct_optimizer() {
    use terse::optimizers::OptimizerRegistry;

    let registry = OptimizerRegistry::new();

    // Git commands handled
    assert!(registry.can_handle("git status"));
    assert!(registry.can_handle("cd /repo && git diff"));

    // File commands handled
    assert!(registry.can_handle("ls -la"));
    assert!(registry.can_handle("find . -name '*.rs'"));
    assert!(registry.can_handle("cat README.md"));

    // Build commands handled
    assert!(registry.can_handle("cargo test"));
    assert!(registry.can_handle("npm install"));
    assert!(registry.can_handle("cargo clippy"));

    // Docker commands handled
    assert!(registry.can_handle("docker ps"));
    assert!(registry.can_handle("docker images"));

    // Unknown commands handled by generic optimizer
    assert!(registry.can_handle("some-unknown-tool --flag"));
}

#[test]
fn registry_git_has_priority_over_generic() {
    use terse::optimizers::OptimizerRegistry;

    let registry = OptimizerRegistry::new();

    // Git diff with sample output — should use git optimizer, not generic
    let result = registry.optimize_first(
        "git diff",
        "diff --git a/file.rs b/file.rs\n--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old\n+new\n",
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().optimizer_used, "git");
}

#[test]
fn registry_build_has_priority_over_generic() {
    use terse::optimizers::OptimizerRegistry;

    let registry = OptimizerRegistry::new();
    let result = registry.optimize_first(
        "cargo test",
        "running 1 test\ntest mytest ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().optimizer_used, "build");
}

#[test]
fn registry_disabled_optimizer_not_loaded() {
    use terse::config::schema::OptimizersConfig;
    use terse::optimizers::OptimizerRegistry;

    let mut cfg = OptimizersConfig::default();
    cfg.git.enabled = false;

    let registry = OptimizerRegistry::from_config(&cfg);

    // Git commands should no longer be handled by the git optimizer
    // (generic will still catch them as a fallback)
    let result = registry.optimize_first(
        "git status",
        "On branch main\nnothing to commit, working tree clean",
    );
    assert!(result.is_some());
    // Should fall through to generic, not git
    assert_ne!(result.unwrap().optimizer_used, "git");
}

#[test]
fn registry_all_disabled_except_generic() {
    use terse::config::schema::OptimizersConfig;
    use terse::optimizers::OptimizerRegistry;

    let mut cfg = OptimizersConfig::default();
    cfg.git.enabled = false;
    cfg.file.enabled = false;
    cfg.build.enabled = false;
    cfg.docker.enabled = false;

    let registry = OptimizerRegistry::from_config(&cfg);

    // Everything falls through to generic
    let result = registry.optimize_first("git status", "On branch main\nnothing to commit");
    assert!(result.is_some());
    assert_eq!(result.unwrap().optimizer_used, "generic");
}

#[test]
fn registry_all_disabled_returns_none() {
    use terse::config::schema::OptimizersConfig;
    use terse::optimizers::OptimizerRegistry;

    let mut cfg = OptimizersConfig::default();
    cfg.git.enabled = false;
    cfg.file.enabled = false;
    cfg.build.enabled = false;
    cfg.docker.enabled = false;
    cfg.generic.enabled = false;

    let registry = OptimizerRegistry::from_config(&cfg);

    // Nothing can handle it
    assert!(!registry.can_handle("git status"));
    assert!(registry.optimize_first("git status", "output").is_none());
}
