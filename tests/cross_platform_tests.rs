/// Cross-platform test suite (Phase 10).
///
/// Validates that platform-specific code paths work correctly on the
/// current host. Tests are structured so they pass on all three targets
/// (Windows, macOS, Linux) — assertions adapt via `cfg!()`.
use terse::matching::extract_core_command;
use terse::optimizers::{
    BuildOptimizer, CommandContext, FileOptimizer, GenericOptimizer, Optimizer,
};
use terse::router::decision::{HookDecision, PassthroughReason};
use terse::safety::classifier::{self, CommandClass};
use terse::utils::process;

// ===========================================================================
// 1. Platform utilities (src/utils/process.rs)
// ===========================================================================

#[test]
fn platform_name_matches_cfg() {
    let name = process::platform_name();
    if cfg!(target_os = "windows") {
        assert_eq!(name, "windows");
    } else if cfg!(target_os = "macos") {
        assert_eq!(name, "macos");
    } else if cfg!(target_os = "linux") {
        assert_eq!(name, "linux");
    } else {
        assert_eq!(name, "unknown");
    }
}

#[test]
fn default_shell_is_platform_appropriate() {
    let shell = process::default_shell();
    if cfg!(target_os = "windows") {
        assert_eq!(shell, "pwsh");
    } else {
        assert_eq!(shell, "sh");
    }
}

#[test]
fn terse_binary_name_has_correct_extension() {
    let name = process::terse_binary_name();
    if cfg!(target_os = "windows") {
        assert!(name.ends_with(".exe"), "expected .exe suffix on Windows");
    } else {
        assert!(!name.contains('.'), "expected no extension on Unix");
    }
}

#[test]
fn path_normalization_uses_native_separator() {
    let normalized = process::normalize_path_separator("a/b\\c");
    if cfg!(target_os = "windows") {
        assert_eq!(normalized, "a\\b\\c");
    } else {
        assert_eq!(normalized, "a/b/c");
    }
}

#[test]
fn display_path_always_uses_forward_slash() {
    assert_eq!(process::to_display_path("a\\b\\c"), "a/b/c");
    assert_eq!(process::to_display_path("x/y/z"), "x/y/z");
}

#[test]
fn home_directories_are_populated() {
    assert!(process::terse_home_dir().is_some());
    assert!(process::terse_bin_dir().is_some());
    assert!(process::claude_settings_path().is_some());
}

#[test]
fn shell_command_echo_works() {
    let out = process::run_shell_command("echo cross-platform-test")
        .expect("echo should succeed on all platforms");
    assert!(out.success);
    assert!(
        out.stdout.contains("cross-platform-test"),
        "stdout was: {}",
        out.stdout
    );
}

#[test]
fn is_command_available_finds_known_binary() {
    if cfg!(target_os = "windows") {
        assert!(process::is_command_available("cmd"));
    } else {
        assert!(process::is_command_available("sh"));
    }
}

#[test]
fn is_command_available_rejects_bogus_binary() {
    assert!(!process::is_command_available(
        "nonexistent_binary_xyz_12345"
    ));
}

// ===========================================================================
// 2. Matching engine — Windows shell wrapper extraction
// ===========================================================================

#[test]
fn matching_unwraps_cmd_c_wrapper() {
    let core = extract_core_command("cmd /c git status");
    assert_eq!(core, "git status");
}

#[test]
fn matching_unwraps_cmd_exe_c_wrapper() {
    let core = extract_core_command("cmd.exe /c echo hello");
    assert_eq!(core, "echo hello");
}

#[test]
fn matching_unwraps_pwsh_command_wrapper() {
    let core = extract_core_command("pwsh -Command git diff");
    assert_eq!(core, "git diff");
}

#[test]
fn matching_unwraps_pwsh_exe_c_wrapper() {
    let core = extract_core_command("pwsh.exe -c cargo test");
    assert_eq!(core, "cargo test");
}

#[test]
fn matching_unwraps_powershell_command_wrapper() {
    let core = extract_core_command("powershell -Command ls -la");
    assert_eq!(core, "ls -la");
}

#[test]
fn matching_unwraps_powershell_exe_c_wrapper() {
    let core = extract_core_command("powershell.exe -c npm test");
    assert_eq!(core, "npm test");
}

#[test]
fn matching_unwraps_pwsh_noprofile_wrapper() {
    let core = extract_core_command("pwsh -NoProfile -Command git log --oneline");
    assert_eq!(core, "git log --oneline");
}

#[test]
fn matching_case_insensitive_shell_wrappers() {
    assert_eq!(extract_core_command("CMD /C git status"), "git status");
    assert_eq!(extract_core_command("PWSH -COMMAND git diff"), "git diff");
    assert_eq!(
        extract_core_command("PowerShell.exe -C cargo build"),
        "cargo build"
    );
}

#[test]
fn matching_preserves_unix_shell_wrappers() {
    assert_eq!(extract_core_command("bash -c 'git status'"), "git status");
    assert_eq!(extract_core_command("sh -c 'echo hello'"), "echo hello");
}

#[test]
fn matching_strips_cd_before_windows_wrapper() {
    let core = extract_core_command("cd /d C:\\repo && cmd /c git status");
    assert!(core.contains("git status"), "core was: {core}",);
}

// ===========================================================================
// 3. Safety classifier — Windows destructive commands
// ===========================================================================

#[test]
fn classifier_blocks_windows_cmd_destructive() {
    // Windows cmd.exe destructive commands
    let cmds = [
        "del file.txt",
        "erase file.txt",
        "rd /s folder",
        "ren old new",
    ];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::NeverOptimize,
            "expected NeverOptimize for: {cmd}"
        );
    }
}

#[test]
fn classifier_blocks_powershell_destructive_cmdlets() {
    let cmds = [
        "Remove-Item file.txt",
        "Move-Item a b",
        "Rename-Item old new",
    ];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::NeverOptimize,
            "expected NeverOptimize for: {cmd}"
        );
    }
}

#[test]
fn classifier_blocks_powershell_aliases() {
    let cmds = ["ri file.txt", "mi a b"];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::NeverOptimize,
            "expected NeverOptimize for: {cmd}"
        );
    }
}

#[test]
fn classifier_blocks_powershell_file_writing() {
    let cmds = [
        "Set-Content -Path f.txt -Value hi",
        "Out-File -FilePath output.txt",
        "Add-Content log.txt more",
    ];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::NeverOptimize,
            "expected NeverOptimize for: {cmd}"
        );
    }
}

#[test]
fn classifier_blocks_windows_copy_commands() {
    let cmds = ["copy a b", "xcopy /s src dst", "robocopy src dst"];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::NeverOptimize,
            "expected NeverOptimize for: {cmd}"
        );
    }
}

#[test]
fn classifier_allows_safe_windows_commands() {
    let cmds = ["dir", "type file.txt", "echo hello", "where git"];
    for cmd in cmds {
        assert_eq!(
            classifier::classify(cmd),
            CommandClass::Optimizable,
            "expected Optimizable for: {cmd}"
        );
    }
}

// ===========================================================================
// 4. File optimizer — platform-aware `find`
// ===========================================================================

#[test]
fn file_optimizer_find_is_platform_aware() {
    let opt = FileOptimizer::new();
    let ctx = CommandContext::new("find . -name '*.rs'");

    if cfg!(target_os = "windows") {
        // On Windows, `find` is a text-search tool — should NOT be handled
        assert!(
            !opt.can_handle(&ctx),
            "find should be skipped on Windows (Windows find is text-search)"
        );
    } else {
        // On Unix, `find` is a file-search tool — should be handled
        assert!(opt.can_handle(&ctx), "find should be handled on Unix");
    }
}

#[test]
fn file_optimizer_handles_ls_everywhere() {
    let opt = FileOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("ls -la")));
    assert!(opt.can_handle(&CommandContext::new("ls src/")));
}

#[test]
fn file_optimizer_handles_cat_everywhere() {
    let opt = FileOptimizer::new();
    assert!(opt.can_handle(&CommandContext::new("cat README.md")));
    assert!(opt.can_handle(&CommandContext::new("cat package.json")));
}

// ===========================================================================
// 5. Build optimizer — Windows build tools
// ===========================================================================

#[test]
fn build_optimizer_handles_nmake() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("nmake test");
    assert!(
        opt.can_handle(&ctx),
        "nmake test should be recognized as a build/test command"
    );
}

#[test]
fn build_optimizer_handles_nuget_restore() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("nuget restore solution.sln");
    assert!(
        opt.can_handle(&ctx),
        "nuget restore should be recognized as a build command"
    );
}

#[test]
fn build_optimizer_handles_cross_platform_tools() {
    let opt = BuildOptimizer::new();
    // These should work on all platforms
    assert!(opt.can_handle(&CommandContext::new("cargo test")));
    assert!(opt.can_handle(&CommandContext::new("npm test")));
    assert!(opt.can_handle(&CommandContext::new("make test")));
    assert!(opt.can_handle(&CommandContext::new("dotnet build")));
}

// ===========================================================================
// 6. Generic optimizer — cross-platform commands
// ===========================================================================

#[test]
fn generic_optimizer_handles_common_commands() {
    let opt = GenericOptimizer::new();

    // These informational commands should be optimizable everywhere
    assert!(opt.can_handle(&CommandContext::new("echo hello")));
    assert!(opt.can_handle(&CommandContext::new("env")));
}

// ===========================================================================
// 7. End-to-end: Windows shell wrappers → optimizer matching
// ===========================================================================

#[test]
fn optimizer_matches_through_cmd_wrapper() {
    let opt = terse::optimizers::GitOptimizer::new();
    let ctx = CommandContext::new("cmd /c git status");
    assert!(
        opt.can_handle(&ctx),
        "git optimizer should match through cmd /c wrapper"
    );
}

#[test]
fn optimizer_matches_through_pwsh_wrapper() {
    let opt = terse::optimizers::GitOptimizer::new();
    let ctx = CommandContext::new("pwsh -Command git log --oneline -20");
    assert!(
        opt.can_handle(&ctx),
        "git optimizer should match through pwsh -Command wrapper"
    );
}

#[test]
fn optimizer_matches_through_powershell_wrapper() {
    let opt = terse::optimizers::GitOptimizer::new();
    let ctx = CommandContext::new("powershell.exe -c git diff HEAD~3");
    assert!(
        opt.can_handle(&ctx),
        "git optimizer should match through powershell.exe wrapper"
    );
}

#[test]
fn build_optimizer_matches_through_cmd_wrapper() {
    let opt = BuildOptimizer::new();
    let ctx = CommandContext::new("cmd /c cargo test -- --nocapture");
    assert!(
        opt.can_handle(&ctx),
        "build optimizer should match through cmd /c wrapper"
    );
}

#[test]
fn file_optimizer_matches_through_pwsh_wrapper() {
    let opt = FileOptimizer::new();
    let ctx = CommandContext::new("pwsh -Command ls -la");
    assert!(
        opt.can_handle(&ctx),
        "file optimizer should match through pwsh -Command wrapper"
    );
}

// ===========================================================================
// 8. Router decisions — platform consistency
// ===========================================================================

#[test]
fn router_passthrough_for_windows_destructive_via_hook() {
    // Destructive Windows commands should result in passthrough via decide_hook
    let decision = terse::router::decide_hook("del important.dat");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::NeverOptimize);
        }
        other => panic!("expected Passthrough for 'del', got: {other:?}"),
    }
}

#[test]
fn router_passthrough_for_powershell_remove_item() {
    let decision = terse::router::decide_hook("Remove-Item -Recurse folder");
    match decision {
        HookDecision::Passthrough(reason) => {
            assert_eq!(reason, PassthroughReason::NeverOptimize);
        }
        other => panic!("expected Passthrough for Remove-Item, got: {other:?}"),
    }
}

#[test]
fn router_optimizes_safe_command_regardless_of_platform() {
    // A simple git status should always be optimizable
    let decision = terse::router::decide_hook("git status");
    match decision {
        HookDecision::Rewrite => {
            // Expected
        }
        other => panic!("expected Rewrite for 'git status', got: {other:?}"),
    }
}
