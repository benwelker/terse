use terse::matching::{extract_core_command, is_terse_invocation};
use terse::optimizers::{GitOptimizer, Optimizer};

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — basic commands
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_handles_git_commands() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("git status"));
    assert!(optimizer.can_handle("cd /repo && git diff"));
    assert!(!optimizer.can_handle("cargo test"));
}

#[test]
fn git_optimizer_handles_short_status_commands() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("git push"));
    assert!(optimizer.can_handle("git pull"));
    assert!(optimizer.can_handle("git fetch"));
    assert!(optimizer.can_handle("git add ."));
    assert!(optimizer.can_handle("git commit -m \"test\""));
}

#[test]
fn git_optimizer_rejects_non_git_commands() {
    let optimizer = GitOptimizer::new();

    assert!(!optimizer.can_handle("cargo test"));
    assert!(!optimizer.can_handle("npm install"));
    assert!(!optimizer.can_handle("echo 'git status'"));
}

// ---------------------------------------------------------------------------
// GitOptimizer can_handle — wrapped / prefixed commands via matching engine
// ---------------------------------------------------------------------------

#[test]
fn git_optimizer_handles_env_var_prefix() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("LANG=C git diff"));
    assert!(optimizer.can_handle("GIT_PAGER=cat git log"));
    assert!(optimizer.can_handle("LANG=C GIT_DIR=/foo git status"));
}

#[test]
fn git_optimizer_handles_subshell() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("(cd /repo && git status)"));
    assert!(optimizer.can_handle("(git diff)"));
}

#[test]
fn git_optimizer_handles_shell_wrapper() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("bash -c \"git status\""));
    assert!(optimizer.can_handle("sh -c 'git diff'"));
}

#[test]
fn git_optimizer_handles_pipeline() {
    let optimizer = GitOptimizer::new();

    // The core command before the pipe is "git log" which is optimizable
    assert!(optimizer.can_handle("git log | head -20"));
}

#[test]
fn git_optimizer_handles_complex_cd_env_combo() {
    let optimizer = GitOptimizer::new();

    assert!(optimizer.can_handle("cd /home/user && LANG=C git status"));
    assert!(optimizer.can_handle("cd /project && GIT_DIR=/foo git diff --cached"));
}

#[test]
fn git_optimizer_preserves_args_through_matching() {
    let optimizer = GitOptimizer::new();

    // Ensure --format=oneline is NOT stripped as an env var
    assert!(optimizer.can_handle("cd /repo && git log --format=oneline"));
}

// ---------------------------------------------------------------------------
// Matching engine integration: extract_core_command
// ---------------------------------------------------------------------------

#[test]
fn matching_extracts_core_from_cd_chain() {
    assert_eq!(
        extract_core_command("cd /repo && git status"),
        "git status"
    );
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
    assert!(!is_terse_invocation("cd /my-terse-run-project && git status"));
    assert!(!is_terse_invocation("terse stats"));
}
