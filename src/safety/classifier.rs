/// Command classification for routing decisions.
///
/// Classifies commands as either safe to optimize or requiring passthrough.
/// Destructive commands (rm, mv) and interactive editors (vim, code) are never
/// optimized. Commands with file output redirections (`>`, `>>`) are also
/// passed through since the redirect consumes stdout and there is nothing
/// useful for terse to capture.
///
/// The classifier examines both the **core command** (extracted by the
/// matching engine) for command-name checks, and the **full original command**
/// for redirect detection.
use crate::config;
use crate::matching;

/// Classification result for a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Never optimize — destructive, interactive, or redirect-based command.
    NeverOptimize,
    /// Safe for optimization — the router may apply fast or smart path.
    Optimizable,
}

/// Built-in commands that must never be optimized, matched by first word of
/// the core command.
///
/// These are the minimum set that is always enforced even if the user's config
/// removes them. Additional commands can be added via `passthrough.commands`
/// in config.toml.
const BUILTIN_PASSTHROUGH_COMMANDS: &[&str] = &[
    // Unix destructive commands
    "rm",
    "rmdir",
    "mv",
    // Windows cmd destructive commands
    "del",
    "erase",
    "rd",
    "ren",
    "move",
    "copy",
    "xcopy",
    "robocopy",
    // PowerShell destructive cmdlets
    "remove-item",
    "move-item",
    "rename-item",
    // PowerShell aliases for destructive ops
    "ri",
    "mi",
    // PowerShell file-writing cmdlets (act like redirects)
    "set-content",
    "out-file",
    "add-content",
    // Editors (Unix)
    "vim",
    "vi",
    "nano",
    "emacs",
    // Editors (cross-platform / Windows)
    "code",
    "subl",
    "notepad",
    "notepad++",
];

/// Classify a command for routing purposes.
///
/// `command` is the full original command string as received from Claude Code.
/// Internally, the core command is extracted for name matching, while the
/// full string is checked for file output redirections.
///
/// Passthrough commands are loaded from config (merging with built-in set).
pub fn classify(command: &str) -> CommandClass {
    let core = matching::extract_core_command(command);
    let first = first_word(core);

    // Check built-in passthrough commands (always enforced).
    if BUILTIN_PASSTHROUGH_COMMANDS
        .iter()
        .any(|&cmd| first.eq_ignore_ascii_case(cmd))
    {
        return CommandClass::NeverOptimize;
    }

    // Check config-supplied passthrough commands.
    let cfg = config::load();
    if cfg
        .passthrough
        .commands
        .iter()
        .any(|cmd| first.eq_ignore_ascii_case(cmd))
    {
        return CommandClass::NeverOptimize;
    }

    // Check the full command for output redirections (> or >>).
    if has_file_redirect(command) {
        return CommandClass::NeverOptimize;
    }

    CommandClass::Optimizable
}

/// Extract the first whitespace-delimited word from a string.
fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// Detect unquoted output redirections (`>` or `>>`) in a command string.
///
/// Skips `>` characters inside single or double quotes, and ignores
/// fd-duplication patterns like `>&` / `2>&1` which redirect between file
/// descriptors rather than to files.
fn has_file_redirect(command: &str) -> bool {
    let bytes = command.as_bytes();
    let len = bytes.len();

    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'\'' if !in_double_quote => in_single_quote = !in_single_quote,
            b'"' if !in_single_quote => in_double_quote = !in_double_quote,
            b'>' if !in_single_quote && !in_double_quote => {
                // Skip heredoc / herestring: preceded by `<`
                if i > 0 && bytes[i - 1] == b'<' {
                    i += 1;
                    continue;
                }
                // Skip fd duplication: `>&` (e.g. 2>&1)
                if i + 1 < len && bytes[i + 1] == b'&' {
                    i += 2;
                    continue;
                }
                return true;
            }
            _ => {}
        }
        i += 1;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Command name classification ---

    #[test]
    fn destructive_commands_are_never_optimized() {
        assert_eq!(classify("rm -rf node_modules"), CommandClass::NeverOptimize);
        assert_eq!(classify("rm file.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("mv old.txt new.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("rmdir empty_dir"), CommandClass::NeverOptimize);
    }

    #[test]
    fn editor_commands_are_never_optimized() {
        assert_eq!(classify("vim file.rs"), CommandClass::NeverOptimize);
        assert_eq!(classify("vi file.rs"), CommandClass::NeverOptimize);
        assert_eq!(classify("nano file.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("code ."), CommandClass::NeverOptimize);
        assert_eq!(classify("emacs file.el"), CommandClass::NeverOptimize);
    }

    #[test]
    fn case_insensitive_classification() {
        assert_eq!(classify("RM -rf /tmp/test"), CommandClass::NeverOptimize);
        assert_eq!(classify("VIM file.txt"), CommandClass::NeverOptimize);
    }

    #[test]
    fn safe_commands_are_optimizable() {
        assert_eq!(classify("git status"), CommandClass::Optimizable);
        assert_eq!(classify("ls -la"), CommandClass::Optimizable);
        assert_eq!(classify("cat file.txt"), CommandClass::Optimizable);
        assert_eq!(classify("echo hello"), CommandClass::Optimizable);
        assert_eq!(classify("cargo test"), CommandClass::Optimizable);
    }

    #[test]
    fn wrapped_commands_classified_correctly() {
        assert_eq!(
            classify("cd /repo && rm -rf build"),
            CommandClass::NeverOptimize
        );
        assert_eq!(
            classify("cd /repo && git status"),
            CommandClass::Optimizable
        );
    }

    // --- Redirect detection ---

    #[test]
    fn output_redirect_is_never_optimized() {
        assert_eq!(
            classify("echo hello > file.txt"),
            CommandClass::NeverOptimize
        );
        assert_eq!(classify("ls -la >> log.txt"), CommandClass::NeverOptimize);
    }

    #[test]
    fn redirect_inside_quotes_is_ignored() {
        assert_eq!(
            classify("echo \"hello > world\""),
            CommandClass::Optimizable
        );
        assert_eq!(classify("echo 'data >> more'"), CommandClass::Optimizable);
    }

    #[test]
    fn fd_duplication_is_not_a_redirect() {
        assert_eq!(classify("cmd 2>&1"), CommandClass::Optimizable);
        assert_eq!(classify("cmd >&2"), CommandClass::Optimizable);
    }

    #[test]
    fn heredoc_redirect_is_not_file_redirect() {
        // <<EOF is a heredoc, not an output redirect.
        // (Heredocs are handled separately by the matching engine.)
        assert!(!has_file_redirect("cat <<EOF\nhello\nEOF"));
    }

    // --- Windows-specific commands (Phase 10) ---

    #[test]
    fn windows_destructive_commands_are_never_optimized() {
        assert_eq!(classify("del file.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("erase file.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("rd /s /q mydir"), CommandClass::NeverOptimize);
        assert_eq!(classify("ren old.txt new.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("move a.txt b.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("copy a.txt b.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("xcopy src dst /e"), CommandClass::NeverOptimize);
        assert_eq!(
            classify("robocopy src dst /mir"),
            CommandClass::NeverOptimize
        );
    }

    #[test]
    fn powershell_destructive_cmdlets_are_never_optimized() {
        assert_eq!(
            classify("Remove-Item -Recurse -Force node_modules"),
            CommandClass::NeverOptimize
        );
        assert_eq!(
            classify("Move-Item old.txt new.txt"),
            CommandClass::NeverOptimize
        );
        assert_eq!(
            classify("Rename-Item old.txt new.txt"),
            CommandClass::NeverOptimize
        );
    }

    #[test]
    fn powershell_file_writing_cmdlets_are_never_optimized() {
        assert_eq!(
            classify("Set-Content -Path file.txt -Value hello"),
            CommandClass::NeverOptimize
        );
        assert_eq!(
            classify("Out-File -FilePath log.txt"),
            CommandClass::NeverOptimize
        );
        assert_eq!(
            classify("Add-Content -Path log.txt -Value data"),
            CommandClass::NeverOptimize
        );
    }

    #[test]
    fn powershell_aliases_are_never_optimized() {
        assert_eq!(classify("ri -Force file.txt"), CommandClass::NeverOptimize);
        assert_eq!(classify("mi old.txt new.txt"), CommandClass::NeverOptimize);
    }
}
