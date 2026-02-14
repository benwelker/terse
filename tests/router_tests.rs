use terse::router::decide_hook;
/// Router decision tests (Phase 4).
///
/// Tests the routing logic for hook-level decisions, command classification,
/// and circuit breaker behavior. Tests that require command execution
/// (e.g., optimizer end-to-end) are in `optimizer_tests.rs`.
use terse::router::decision::{DecisionCache, HookDecision, OptimizationPath, PassthroughReason};
use terse::safety::classifier::{self, CommandClass};

// ---------------------------------------------------------------------------
// Classifier tests
// ---------------------------------------------------------------------------

#[test]
fn classifier_rejects_destructive_commands() {
    assert_eq!(
        classifier::classify("rm -rf node_modules"),
        CommandClass::NeverOptimize
    );
    assert_eq!(
        classifier::classify("mv old.txt new.txt"),
        CommandClass::NeverOptimize
    );
    assert_eq!(
        classifier::classify("rmdir empty_dir"),
        CommandClass::NeverOptimize
    );
}

#[test]
fn classifier_rejects_editors() {
    assert_eq!(
        classifier::classify("vim src/main.rs"),
        CommandClass::NeverOptimize
    );
    assert_eq!(classifier::classify("code ."), CommandClass::NeverOptimize);
    assert_eq!(
        classifier::classify("nano ~/.bashrc"),
        CommandClass::NeverOptimize
    );
}

#[test]
fn classifier_rejects_output_redirects() {
    assert_eq!(
        classifier::classify("echo hello > file.txt"),
        CommandClass::NeverOptimize
    );
    assert_eq!(
        classifier::classify("ls >> log.txt"),
        CommandClass::NeverOptimize
    );
}

#[test]
fn classifier_allows_safe_commands() {
    assert_eq!(
        classifier::classify("git status"),
        CommandClass::Optimizable
    );
    assert_eq!(classifier::classify("ls -la"), CommandClass::Optimizable);
    assert_eq!(
        classifier::classify("cargo test"),
        CommandClass::Optimizable
    );
    assert_eq!(
        classifier::classify("cat README.md"),
        CommandClass::Optimizable
    );
}

#[test]
fn classifier_allows_fd_duplication() {
    // 2>&1 is not a file redirect — it's fd duplication
    assert_eq!(
        classifier::classify("cargo test 2>&1"),
        CommandClass::Optimizable
    );
}

#[test]
fn classifier_allows_redirect_inside_quotes() {
    assert_eq!(
        classifier::classify("echo \"hello > world\""),
        CommandClass::Optimizable
    );
}

#[test]
fn classifier_handles_wrapped_destructive_commands() {
    // The matching engine extracts the core command from cd && chains
    assert_eq!(
        classifier::classify("cd /tmp && rm -rf build"),
        CommandClass::NeverOptimize
    );
}

// ---------------------------------------------------------------------------
// Hook decision tests
// ---------------------------------------------------------------------------

#[test]
fn hook_passthrough_for_terse_invocation() {
    let decision = decide_hook("terse run \"git status\"");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::TerseInvocation);
        }
        _ => panic!("expected passthrough for terse invocation"),
    }
}

#[test]
fn hook_passthrough_for_heredoc() {
    let decision = decide_hook("cat <<EOF\nhello world\nEOF");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::Heredoc);
        }
        _ => panic!("expected passthrough for heredoc"),
    }
}

#[test]
fn hook_passthrough_for_destructive_command() {
    let decision = decide_hook("rm -rf /tmp/test");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::NeverOptimize);
        }
        _ => panic!("expected passthrough for destructive command"),
    }
}

#[test]
fn hook_passthrough_for_editor() {
    let decision = decide_hook("vim file.txt");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::NeverOptimize);
        }
        _ => panic!("expected passthrough for editor command"),
    }
}

#[test]
fn hook_passthrough_for_redirects() {
    let decision = decide_hook("echo test > output.txt");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::NeverOptimize);
        }
        _ => panic!("expected passthrough for redirect"),
    }
}

#[test]
fn hook_rewrites_git_commands() {
    let decision = decide_hook("git status");
    match decision {
        HookDecision::Rewrite => {}
        HookDecision::Passthrough(reason) => {
            panic!("expected rewrite for git status, got passthrough: {reason}");
        }
    }
}

#[test]
fn hook_rewrites_wrapped_git_commands() {
    let decision = decide_hook("cd /repo && git log");
    match decision {
        HookDecision::Rewrite => {}
        HookDecision::Passthrough(reason) => {
            panic!("expected rewrite for wrapped git log, got passthrough: {reason}");
        }
    }
}

#[test]
fn hook_rewrites_unknown_safe_commands() {
    // The simplified hook always rewrites safe commands — path decision
    // happens post-execution based on output size.
    let decision = decide_hook("some-unknown-tool --verbose");
    match decision {
        HookDecision::Rewrite => {}
        HookDecision::Passthrough(reason) => {
            panic!("expected rewrite for safe unknown command, got passthrough: {reason}");
        }
    }
}

// ---------------------------------------------------------------------------
// Decision cache tests
// ---------------------------------------------------------------------------

#[test]
fn decision_cache_stores_and_retrieves() {
    let mut cache = DecisionCache::new(300);
    cache.insert("git".to_string(), OptimizationPath::FastPath);
    cache.insert("docker".to_string(), OptimizationPath::SmartPath);

    assert_eq!(cache.get("git"), Some(OptimizationPath::FastPath));
    assert_eq!(cache.get("docker"), Some(OptimizationPath::SmartPath));
    assert_eq!(cache.get("unknown"), None);
}

#[test]
fn decision_cache_entries_expire() {
    let mut cache = DecisionCache::new(0); // 0-second TTL → expires immediately
    cache.insert("git".to_string(), OptimizationPath::FastPath);
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert_eq!(cache.get("git"), None);
}

// ---------------------------------------------------------------------------
// Optimization path display
// ---------------------------------------------------------------------------

#[test]
fn optimization_path_display_names() {
    assert_eq!(OptimizationPath::FastPath.to_string(), "fast");
    assert_eq!(OptimizationPath::SmartPath.to_string(), "smart");
    assert_eq!(OptimizationPath::Passthrough.to_string(), "passthrough");
}

#[test]
fn passthrough_reason_display_is_descriptive() {
    let reason = PassthroughReason::NeverOptimize;
    let display = reason.to_string();
    assert!(
        display.contains("destructive") || display.contains("editor"),
        "expected descriptive reason, got: {display}"
    );
}
