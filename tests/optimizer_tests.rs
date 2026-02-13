use terse::optimizers::{GitOptimizer, Optimizer};

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
