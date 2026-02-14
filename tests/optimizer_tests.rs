use terse::matching::{extract_core_command, is_terse_invocation};
use terse::optimizers::{CommandContext, GitOptimizer, Optimizer};

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
