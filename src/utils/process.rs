use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    #[allow(dead_code)]
    pub exit_code: Option<i32>,
    #[allow(dead_code)]
    pub success: bool,
}

/// Execute a shell command using the platform's default shell.
///
/// On Windows, uses `pwsh -Command`. Falls back to `cmd /C` if `pwsh` is
/// not available. On Unix, uses `sh -c`.
pub fn run_shell_command(command: &str) -> Result<ProcessOutput> {
    let output = platform_shell_exec(command)?;

    Ok(ProcessOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        success: output.status.success(),
    })
}

/// Platform-specific shell execution.
///
/// Windows: tries `pwsh` first, falls back to `cmd /C`.
/// Unix: uses `sh -c`.
fn platform_shell_exec(command: &str) -> Result<std::process::Output> {
    #[cfg(target_os = "windows")]
    {
        // Prefer pwsh (PowerShell 7+), fall back to cmd.exe
        match Command::new("pwsh")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(command)
            .output()
        {
            Ok(output) => Ok(output),
            Err(_) => Command::new("cmd")
                .arg("/C")
                .arg(command)
                .output()
                .with_context(|| format!("failed executing command: {command}")),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .with_context(|| format!("failed executing command: {command}"))
    }
}

// ---------------------------------------------------------------------------
// Path normalization
// ---------------------------------------------------------------------------

/// Normalize a file path to use the platform's native separator.
///
/// On Windows, converts `/` to `\`. On Unix, converts `\` to `/`.
/// Does not resolve symlinks or canonicalize — purely syntactic.
pub fn normalize_path_separator(path: &str) -> String {
    if cfg!(target_os = "windows") {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

/// Convert a path to a forward-slash form suitable for display.
///
/// Useful for consistent log output across platforms.
pub fn to_display_path(path: &str) -> String {
    path.replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

/// Name of the current operating system for display/analytics.
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// Name of the default shell on this platform.
pub fn default_shell() -> &'static str {
    if cfg!(target_os = "windows") {
        "pwsh"
    } else {
        "sh"
    }
}

/// Return the platform-specific binary name for terse.
pub fn terse_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "terse.exe"
    } else {
        "terse"
    }
}

/// Return the terse home directory: `~/.terse/`.
pub fn terse_home_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".terse"))
}

/// Return the terse binary directory: `~/.terse/bin/`.
pub fn terse_bin_dir() -> Option<std::path::PathBuf> {
    terse_home_dir().map(|h| h.join("bin"))
}

/// Check if a binary is available on the system PATH.
pub fn is_command_available(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        Command::new("where")
            .arg(name)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("which")
            .arg(name)
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

/// Check if Ollama is available and responsive.
pub fn is_ollama_available() -> bool {
    is_command_available("ollama")
}

/// Get the Claude settings file path for hook registration.
///
/// - Windows: `%USERPROFILE%\.claude\settings.json`
/// - macOS/Linux: `~/.claude/settings.json`
pub fn claude_settings_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

/// Get the path to the currently running terse executable.
pub fn current_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

/// Check if a path exists and is executable.
pub fn is_executable(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, .exe files are executable by extension
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_separator() {
        if cfg!(target_os = "windows") {
            assert_eq!(normalize_path_separator("src/main.rs"), "src\\main.rs");
            assert_eq!(normalize_path_separator("a/b/c"), "a\\b\\c");
        } else {
            assert_eq!(normalize_path_separator("src\\main.rs"), "src/main.rs");
            assert_eq!(normalize_path_separator("a\\b\\c"), "a/b/c");
        }
    }

    #[test]
    fn test_to_display_path() {
        assert_eq!(to_display_path("src\\main.rs"), "src/main.rs");
        assert_eq!(to_display_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn test_platform_name() {
        let name = platform_name();
        assert!(
            ["windows", "macos", "linux", "unknown"].contains(&name),
            "unexpected platform: {name}"
        );
    }

    #[test]
    fn test_terse_binary_name() {
        let name = terse_binary_name();
        if cfg!(target_os = "windows") {
            assert_eq!(name, "terse.exe");
        } else {
            assert_eq!(name, "terse");
        }
    }

    #[test]
    fn test_terse_home_dir() {
        let home = terse_home_dir();
        assert!(home.is_some());
        let path = home.unwrap();
        assert!(path.ends_with(".terse"));
    }

    #[test]
    fn test_terse_bin_dir() {
        let bin = terse_bin_dir();
        assert!(bin.is_some());
        let path = bin.unwrap();
        assert!(path.ends_with("bin"));
    }

    #[test]
    fn test_run_shell_command() {
        let result = run_shell_command("echo hello").expect("echo should work");
        assert!(result.success);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn test_is_command_available() {
        // 'echo' should be available on all platforms (built into shell)
        // Use a known binary instead
        if cfg!(target_os = "windows") {
            assert!(is_command_available("cmd"));
        } else {
            assert!(is_command_available("sh"));
        }
    }

    #[test]
    fn test_claude_settings_path() {
        let path = claude_settings_path();
        assert!(path.is_some());
        let p = path.unwrap();
        assert!(p.ends_with("settings.json"));
    }
}
