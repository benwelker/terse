//! CLI command implementations for terse analytics and diagnostics.
//!
//! Provides subcommand handlers for:
//! - `terse stats` — token savings summary, path distribution, top commands
//! - `terse analyze --days N` — time-based trend analysis
//! - `terse discover` — find high-frequency unoptimized commands
//! - `terse health` — check Ollama, config, hook registration
//! - `terse test "command"` — preview optimization pipeline
//! - `terse config show|init|set|reset` — configuration management

use anyhow::{Context, Result};
use colored::Colorize;

use crate::analytics::logger;
use crate::analytics::reporter::{self, DiscoveryCandidate, Stats, TrendEntry};
use crate::config;
use crate::llm::config::SmartPathConfig;
use crate::llm::ollama::OllamaClient;
use crate::router;
use crate::safety::circuit_breaker::CircuitBreaker;
use crate::utils::process;

/// Output format for analytics commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

impl OutputFormat {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s {
            Some("json") => Self::Json,
            Some("csv") => Self::Csv,
            _ => Self::Table,
        }
    }
}

// ---------------------------------------------------------------------------
// terse stats
// ---------------------------------------------------------------------------

/// Show token savings statistics.
pub fn run_stats(format: OutputFormat, days: Option<u32>) -> Result<()> {
    let stats = reporter::compute_stats(days);

    if stats.total_commands == 0 {
        println!(
            "{}",
            "No data yet. Run some commands through terse to see stats.".yellow()
        );
        return Ok(());
    }

    match format {
        OutputFormat::Json => print_stats_json(&stats)?,
        OutputFormat::Csv => print_stats_csv(&stats),
        OutputFormat::Table => print_stats_table(&stats),
    }

    Ok(())
}

fn print_stats_table(stats: &Stats) {
    println!("{}", "terse Token Savings Report".bold().cyan());
    println!("{}", "=".repeat(60));
    println!();

    // Summary
    let saved = stats
        .total_original_tokens
        .saturating_sub(stats.total_optimized_tokens);
    println!("  {} {}", "Total commands:".bold(), stats.total_commands);
    println!("  {} {}", "Tokens saved:  ".bold(), format_number(saved));
    println!(
        "  {} {:.2}%",
        "Avg savings:   ".bold(),
        stats.total_savings_pct
    );
    println!();

    // Path distribution
    let dist = &stats.path_distribution;
    println!("{}", "Path Distribution".bold().cyan());
    println!(
        "  Fast: {} ({:.0}%)  Smart: {} ({:.0}%)  Passthrough: {} ({:.0}%)",
        dist.fast,
        dist.pct(dist.fast),
        dist.smart,
        dist.pct(dist.smart),
        dist.passthrough,
        dist.pct(dist.passthrough),
    );
    println!();

    // Top commands table
    if !stats.command_stats.is_empty() {
        println!("{}", "Top Commands by Token Savings".bold().cyan());
        println!(
            "  {:<20} {:>6} {:>12} {:>10} Optimizer",
            "Command", "Count", "Tokens", "Savings"
        );
        println!("  {}", "-".repeat(58));

        for (i, cmd) in stats.command_stats.iter().take(15).enumerate() {
            let saved = cmd
                .total_original_tokens
                .saturating_sub(cmd.total_optimized_tokens);
            let line = format!(
                "  {:<20} {:>6} {:>12} {:>9.2}% {}",
                truncate(&cmd.command, 20),
                cmd.count,
                format_number(saved),
                cmd.avg_savings_pct,
                cmd.primary_optimizer,
            );

            if i % 2 == 0 {
                println!("{}", line);
            } else {
                println!("{}", line.dimmed());
            }
        }
    }
}

fn print_stats_json(stats: &Stats) -> Result<()> {
    let value = serde_json::json!({
        "total_commands": stats.total_commands,
        "total_original_tokens": stats.total_original_tokens,
        "total_optimized_tokens": stats.total_optimized_tokens,
        "total_savings_pct": stats.total_savings_pct,
        "path_distribution": {
            "fast": stats.path_distribution.fast,
            "smart": stats.path_distribution.smart,
            "passthrough": stats.path_distribution.passthrough,
        },
        "commands": stats.command_stats.iter().map(|c| serde_json::json!({
            "command": c.command,
            "count": c.count,
            "total_original_tokens": c.total_original_tokens,
            "total_optimized_tokens": c.total_optimized_tokens,
            "avg_savings_pct": c.avg_savings_pct,
            "primary_optimizer": c.primary_optimizer,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn print_stats_csv(stats: &Stats) {
    println!("command,count,original_tokens,optimized_tokens,avg_savings_pct,optimizer");
    for cmd in &stats.command_stats {
        println!(
            "{},{},{},{},{:.2},{}",
            cmd.command,
            cmd.count,
            cmd.total_original_tokens,
            cmd.total_optimized_tokens,
            cmd.avg_savings_pct,
            cmd.primary_optimizer,
        );
    }
}

// ---------------------------------------------------------------------------
// terse analyze
// ---------------------------------------------------------------------------

/// Show time-based trend analysis.
pub fn run_analyze(days: u32, format: OutputFormat) -> Result<()> {
    let trends = reporter::compute_trends(days);

    if trends.is_empty() {
        println!("{}", format!("No data in the last {} days.", days).yellow());
        return Ok(());
    }

    match format {
        OutputFormat::Json => print_trends_json(&trends)?,
        OutputFormat::Csv => print_trends_csv(&trends),
        OutputFormat::Table => print_trends_table(&trends, days),
    }

    Ok(())
}

fn print_trends_table(trends: &[TrendEntry], days: u32) {
    println!(
        "{}",
        format!("terse Trends — Last {} Days", days).bold().cyan()
    );
    println!("{}", "=".repeat(50));
    println!(
        "  {:<12} {:>8} {:>12} {:>10}",
        "Date", "Commands", "Saved", "Avg %"
    );
    println!("  {}", "-".repeat(48));

    for entry in trends {
        println!(
            "  {:<12} {:>8} {:>12} {:>9.2}%",
            entry.date,
            entry.commands,
            format_number(entry.tokens_saved),
            entry.avg_savings_pct,
        );
    }
}

fn print_trends_json(trends: &[TrendEntry]) -> Result<()> {
    let values: Vec<_> = trends
        .iter()
        .map(|t| {
            serde_json::json!({
                "date": t.date,
                "commands": t.commands,
                "tokens_saved": t.tokens_saved,
                "avg_savings_pct": t.avg_savings_pct,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

fn print_trends_csv(trends: &[TrendEntry]) {
    println!("date,commands,tokens_saved,avg_savings_pct");
    for t in trends {
        println!(
            "{},{},{},{:.2}",
            t.date, t.commands, t.tokens_saved, t.avg_savings_pct,
        );
    }
}

// ---------------------------------------------------------------------------
// terse discover
// ---------------------------------------------------------------------------

/// Find high-frequency unoptimized commands.
pub fn run_discover(format: OutputFormat, days: Option<u32>) -> Result<()> {
    let candidates = reporter::discover_candidates(days);

    if candidates.is_empty() {
        println!(
            "{}",
            "No discovery candidates — all commands are using the fast path or there is no data."
                .green()
        );
        return Ok(());
    }

    match format {
        OutputFormat::Json => print_discover_json(&candidates)?,
        OutputFormat::Csv => print_discover_csv(&candidates),
        OutputFormat::Table => print_discover_table(&candidates),
    }

    Ok(())
}

fn print_discover_table(candidates: &[DiscoveryCandidate]) {
    println!(
        "{}",
        "Optimizer Candidates — Not on Fast Path".bold().cyan()
    );
    println!("{}", "=".repeat(60));
    println!(
        "  {:<20} {:>6} {:>12} {:>10} Path",
        "Command", "Count", "Tokens", "Avg Tkns"
    );
    println!("  {}", "-".repeat(58));

    for candidate in candidates.iter().take(20) {
        println!(
            "  {:<20} {:>6} {:>12} {:>10} {}",
            truncate(&candidate.command, 20),
            candidate.count,
            format_number(candidate.total_tokens),
            format_number(candidate.avg_tokens),
            candidate.current_path,
        );
    }

    println!();
    println!(
        "  {}",
        "Build rule-based optimizers for the top commands to move them to the fast path.".dimmed()
    );
}

fn print_discover_json(candidates: &[DiscoveryCandidate]) -> Result<()> {
    let values: Vec<_> = candidates
        .iter()
        .map(|c| {
            serde_json::json!({
                "command": c.command,
                "count": c.count,
                "total_tokens": c.total_tokens,
                "avg_tokens": c.avg_tokens,
                "current_path": c.current_path,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

fn print_discover_csv(candidates: &[DiscoveryCandidate]) {
    println!("command,count,total_tokens,avg_tokens,current_path");
    for c in candidates {
        println!(
            "{},{},{},{},{}",
            c.command, c.count, c.total_tokens, c.avg_tokens, c.current_path,
        );
    }
}

// ---------------------------------------------------------------------------
// terse health
// ---------------------------------------------------------------------------

/// Check system health: Ollama, config, circuit breaker, log file.
pub fn run_health() -> Result<()> {
    println!("{}", "terse Health Check".bold().cyan());
    println!("{}", "=".repeat(40));

    // Platform info
    print_health_item(
        "Platform",
        true,
        &format!(
            "{} (shell: {}, binary: {})",
            process::platform_name(),
            process::default_shell(),
            process::terse_binary_name(),
        ),
    );

    // Terse home directory
    let home_ok = process::terse_home_dir()
        .map(|p| p.exists())
        .unwrap_or(false);
    print_health_item(
        "Terse home",
        home_ok,
        &process::terse_home_dir()
            .map(|p| process::to_display_path(&p.to_string_lossy()))
            .unwrap_or_else(|| "unknown".to_string()),
    );

    // Claude settings
    let claude_ok = process::claude_settings_path()
        .map(|p| p.exists())
        .unwrap_or(false);
    print_health_item(
        "Claude settings",
        claude_ok,
        if claude_ok {
            "found"
        } else {
            "not found (run install script to register hook)"
        },
    );

    // 0. Config file status
    let global_exists = config::global_config_file()
        .map(|p| p.exists())
        .unwrap_or(false);
    let project_exists = config::project_config_file()
        .map(|p| p.exists())
        .unwrap_or(false);
    let cfg = config::load();
    print_health_item(
        "Global config",
        global_exists,
        if global_exists {
            "~/.terse/config.toml found"
        } else {
            "not found (run `terse config init` to create)"
        },
    );
    print_health_item(
        "Project config",
        project_exists,
        if project_exists {
            ".terse.toml found"
        } else {
            "none (optional)"
        },
    );
    print_health_item(
        "Mode / Profile",
        true,
        &format!("{:?} / {:?}", cfg.general.mode, cfg.general.profile),
    );
    if cfg.general.safe_mode {
        print_health_item("Safe mode", false, "ON — no optimizations applied");
    }

    // 1. Smart path config
    let smart_config = SmartPathConfig::load();
    print_health_item(
        "Smart path",
        smart_config.enabled,
        if smart_config.enabled {
            "enabled"
        } else {
            "disabled (set TERSE_SMART_PATH=1 to enable)"
        },
    );

    if smart_config.enabled {
        // 2. Ollama binary & connectivity
        let ollama_on_path = process::is_ollama_available();
        let client = OllamaClient::from_config(&smart_config);
        let ollama_ok = ollama_on_path && client.is_healthy();
        let ollama_detail = if ollama_ok {
            format!("reachable at {}", smart_config.ollama_url)
        } else if !ollama_on_path {
            "ollama binary not found on PATH".to_string()
        } else {
            "not reachable — is Ollama running?".to_string()
        };
        print_health_item("Ollama", ollama_ok, &ollama_detail);

        // 3. Model
        print_health_item("Model", true, &smart_config.model);
    }

    // 4. Circuit breaker
    let cb = CircuitBreaker::load();
    let fast_ok = cb.is_allowed(crate::safety::circuit_breaker::PathId::FastPath);
    let smart_ok = cb.is_allowed(crate::safety::circuit_breaker::PathId::SmartPath);
    print_health_item(
        "Circuit breaker (fast)",
        fast_ok,
        if fast_ok { "open" } else { "tripped" },
    );
    print_health_item(
        "Circuit breaker (smart)",
        smart_ok,
        if smart_ok { "open" } else { "tripped" },
    );

    // 5. Log file
    let log_exists = logger::command_log_path()
        .map(|p| p.exists())
        .unwrap_or(false);
    let log_entries = if log_exists {
        logger::read_all_entries().len()
    } else {
        0
    };
    print_health_item(
        "Command log",
        log_exists,
        &if log_exists {
            format!("{} entries", log_entries)
        } else {
            "no log file yet".to_string()
        },
    );

    // 6. Key tool availability
    let git_ok = process::is_command_available("git");
    print_health_item("Git", git_ok, if git_ok { "found" } else { "not found" });

    // 7. Binary location
    if let Some(exe) = process::current_exe_path() {
        let display = process::to_display_path(&exe.to_string_lossy());
        let valid = process::is_executable(&exe);
        print_health_item("Binary", valid, &display);
    }

    if let Some(bin_dir) = process::terse_bin_dir() {
        let norm = process::normalize_path_separator(&bin_dir.to_string_lossy());
        let in_bin = bin_dir.exists();
        print_health_item(
            "Install dir",
            in_bin,
            &format!("{}{}", norm, if in_bin { "" } else { " (not created)" }),
        );
    }

    // 8. Hook registration hint
    println!();
    println!(
        "  {} Check ~/.claude/settings.json for hook registration",
        "Hint:".dimmed()
    );

    Ok(())
}

fn print_health_item(name: &str, ok: bool, detail: &str) {
    let status = if ok {
        "✓".green().bold()
    } else {
        "✗".red().bold()
    };
    println!("  {} {:<25} {}", status, name, detail.dimmed());
}

// ---------------------------------------------------------------------------
// terse config show | init | set | reset
// ---------------------------------------------------------------------------

/// Show the effective (merged) configuration as TOML.
pub fn run_config_show() -> Result<()> {
    let toml_str = config::show_effective_config()?;
    println!("{}", "Effective terse Configuration".bold().cyan());
    println!("{}", "=".repeat(50));
    println!();
    println!("{toml_str}");

    // Show source info
    let global_exists = config::global_config_file()
        .map(|p| p.exists())
        .unwrap_or(false);
    let project_exists = config::project_config_file()
        .map(|p| p.exists())
        .unwrap_or(false);
    println!("{}", "Sources (highest priority last):".dimmed());
    println!("  {} built-in defaults", "·".dimmed());
    if global_exists {
        println!("  {} {}", "✓".green(), "~/.terse/config.toml".dimmed());
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            "~/.terse/config.toml (not found)".dimmed()
        );
    }
    if project_exists {
        println!("  {} {}", "✓".green(), ".terse.toml".dimmed());
    } else {
        println!("  {} {}", "·".dimmed(), ".terse.toml (not found)".dimmed());
    }
    println!(
        "  {} {}",
        "·".dimmed(),
        "TERSE_* environment variables".dimmed()
    );

    Ok(())
}

/// Initialize a default config file at `~/.terse/config.toml`.
pub fn run_config_init(force: bool) -> Result<()> {
    let path = config::init_config(force)?;
    println!(
        "{} Config written to {}",
        "✓".green().bold(),
        path.display()
    );
    println!(
        "  {}",
        "Edit the file to customize terse behavior.".dimmed()
    );

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(bin_dir) = exe_path.parent()
    {
        println!();
        println!("{}", "Quick PATH setup (optional)".bold().cyan());
        println!(
            "  {} {}",
            "Detected terse binary directory:".dimmed(),
            bin_dir.display()
        );

        #[cfg(target_os = "windows")]
        {
            println!("  {}", "For current PowerShell session:".dimmed());
            println!("    $env:Path += \";{}\"", bin_dir.display());
            println!("  {}", "Persist for current user:".dimmed());
            println!("    setx PATH \"$($env:Path);{}\"", bin_dir.display());
        }

        #[cfg(not(target_os = "windows"))]
        {
            println!("  {}", "For current shell session:".dimmed());
            println!("    export PATH=\"$PATH:{}\"", bin_dir.display());
            println!(
                "  {}",
                "Persist (bash/zsh): add this line to your shell profile:".dimmed()
            );
            println!("    export PATH=\"$PATH:{}\"", bin_dir.display());
        }
    }

    Ok(())
}

/// Set a single configuration value in the global config file.
pub fn run_config_set(key: &str, value: &str) -> Result<()> {
    config::set_config_value(key, value)?;
    println!("{} Set {} = {}", "✓".green().bold(), key.bold(), value,);
    Ok(())
}

/// Reset configuration to defaults.
pub fn run_config_reset() -> Result<()> {
    let path = config::reset_config()?;
    println!(
        "{} Config reset to defaults at {}",
        "✓".green().bold(),
        path.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// terse test
// ---------------------------------------------------------------------------

/// Preview the optimization pipeline for a command.
///
/// Shows the hook-level decision, executes the command through the router,
/// and displays the path taken, token savings, and optimized output.
pub fn run_test(command: &str) -> Result<()> {
    let preview = router::preview(command)?;

    println!("{}", "terse Optimization Preview".bold().cyan());
    println!("{}", "=".repeat(50));
    println!("  {} {}", "Command:      ".bold(), command);
    println!("  {} {}", "Hook decision:".bold(), preview.hook_decision);
    println!(
        "  {} {}",
        "Path taken:   ".bold(),
        colorize_path(&preview.execution.path.to_string())
    );
    println!(
        "  {} {}",
        "Optimizer:    ".bold(),
        preview.execution.optimizer_name
    );

    // Preprocessing stats
    if let Some(pp_bytes) = preview.execution.preprocessing_bytes_removed {
        let pp_pct = preview.execution.preprocessing_pct.unwrap_or(0.0);
        let pp_duration = preview.execution.preprocessing_duration_ms.unwrap_or(0);
        println!(
            "  {} {} bytes removed ({:.2}%) in {}ms",
            "Preprocessing:".bold(),
            pp_bytes,
            pp_pct,
            pp_duration,
        );
    }

    // Preprocessing token savings
    if let (Some(tok_before), Some(tok_after)) = (
        preview.execution.preprocessing_tokens_before,
        preview.execution.preprocessing_tokens_after,
    ) {
        let pp_tok_savings = if tok_before == 0 {
            0.0
        } else {
            let saved = tok_before.saturating_sub(tok_after);
            (saved as f64 / tok_before as f64) * 100.0
        };
        println!(
            "  {} {} → {} ({:.2}% savings)",
            "PP tokens:    ".bold(),
            tok_before,
            tok_after,
            pp_tok_savings,
        );
    }

    // Token savings: use preprocessed token count as the baseline so the
    // "Tokens" line reflects only what the optimizer path achieved on top
    // of preprocessing. Falls back to original_tokens when preprocessing
    // metadata is unavailable.
    let tokens_baseline = preview
        .execution
        .preprocessing_tokens_after
        .unwrap_or(preview.execution.original_tokens);

    let savings = if tokens_baseline == 0 {
        0.0
    } else {
        let saved = tokens_baseline.saturating_sub(preview.execution.optimized_tokens);
        (saved as f64 / tokens_baseline as f64) * 100.0
    };

    // Use enough decimal places to avoid "100.0%" when savings are very
    // high but not truly 100%. Cap display at 99.99% unless it's exactly 0.
    let savings_display = if preview.execution.optimized_tokens == 0 {
        savings // genuinely 100%
    } else {
        savings.min(99.99)
    };

    println!(
        "  {} {} → {} ({:.2}% savings)",
        "Tokens:       ".bold(),
        tokens_baseline,
        preview.execution.optimized_tokens,
        savings_display,
    );

    if let Some(latency) = preview.execution.latency_ms {
        println!("  {} {}ms", "Latency:      ".bold(), latency);
    }

    if let Some(ref reason) = preview.execution.fallback_reason {
        println!("  {} {}", "Fallback:     ".bold(), reason.yellow());
    }

    println!();
    println!("{}", "--- Output ---".dimmed());
    print!("{}", preview.execution.output);

    if !preview.execution.stderr.is_empty() {
        println!();
        println!("{}", "--- Stderr ---".dimmed());
        print!("{}", preview.execution.stderr);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format a number with comma separators for readability.
fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

/// Colorize an optimization path name.
fn colorize_path(path: &str) -> colored::ColoredString {
    match path {
        "fast" => path.green(),
        "smart" => path.blue(),
        "passthrough" => path.yellow(),
        _ => path.normal(),
    }
}

// ---------------------------------------------------------------------------
// terse self uninstall
// ---------------------------------------------------------------------------

/// Uninstall terse: deregister hook, remove PATH entry, and delete files.
///
/// Mirrors the logic in `scripts/uninstall.sh` and `scripts/uninstall.ps1`
/// but runs natively — no network, no python3 dependency, no version mismatch.
///
/// Follows the `rustup self uninstall` convention.
pub fn run_self_uninstall(keep_data: bool, force: bool) -> Result<()> {
    use std::io::Write;

    println!("{}", "terse Self-Uninstall".bold().cyan());
    println!("{}", "=".repeat(40));
    println!();

    if keep_data {
        println!(
            "  This will remove the terse binary and hook registration."
        );
        println!(
            "  Config and log files in ~/.terse/ will be {}.",
            "preserved".yellow()
        );
    } else {
        println!(
            "  This will remove {} terse files including config and logs.",
            "ALL".bold().red()
        );
        println!(
            "  Use {} to preserve config and log files.",
            "--keep-data".bold()
        );
    }
    println!();

    // Confirmation prompt (skip if --force or non-interactive)
    if !force {
        print!("  Continue? [y/N] ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let answer = input.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!();
            println!("{}", "  Uninstall cancelled.".yellow());
            return Ok(());
        }
        println!();
    }

    // Step 1: Deregister Claude Code hook
    uninstall_deregister_hook();

    // Step 2: Remove from shell profile / PATH
    uninstall_remove_path();

    // Step 3: Remove files
    uninstall_remove_files(keep_data);

    // Done
    println!();
    println!("{}", "Uninstall complete!".bold().cyan());
    println!();
    if keep_data {
        let home = process::terse_home_dir()
            .map(|p| process::to_display_path(&p.to_string_lossy()))
            .unwrap_or_else(|| "~/.terse".to_string());
        println!("  Data preserved at: {home}");
        println!("  To fully remove:   rm -rf ~/.terse");
    } else {
        println!("  All terse files have been removed.");
    }
    println!();

    Ok(())
}

/// Deregister the terse hook from `~/.claude/settings.json`.
fn uninstall_deregister_hook() {
    println!("{}", "Removing Claude Code hook...".bold());

    let Some(settings_path) = process::claude_settings_path() else {
        println!("  {} No home directory found", "·".dimmed());
        return;
    };

    if !settings_path.exists() {
        println!(
            "  {} {}",
            "✓".green(),
            "No Claude settings file (nothing to remove)"
        );
        return;
    }

    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => {
            println!(
                "  {} Could not read settings: {}",
                "⚠".yellow(),
                e
            );
            return;
        }
    };

    let mut settings: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            println!(
                "  {} Could not parse settings JSON: {}",
                "⚠".yellow(),
                e
            );
            return;
        }
    };

    // Navigate to hooks.PreToolUse array
    let modified = if let Some(hooks) = settings.get_mut("hooks") {
        if let Some(pre) = hooks.get_mut("PreToolUse") {
            if let Some(arr) = pre.as_array_mut() {
                let original_len = arr.len();
                arr.retain(|entry| !is_terse_hook_entry(entry));
                let removed = original_len - arr.len();

                if removed > 0 {
                    // Clean up empty PreToolUse array
                    if arr.is_empty() {
                        if let Some(hooks_obj) = hooks.as_object_mut() {
                            hooks_obj.remove("PreToolUse");
                            // Clean up empty hooks object
                            if hooks_obj.is_empty() {
                                if let Some(root) = settings.as_object_mut() {
                                    root.remove("hooks");
                                }
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if modified {
        match serde_json::to_string_pretty(&settings) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&settings_path, json) {
                    println!(
                        "  {} Could not write settings: {}",
                        "⚠".yellow(),
                        e
                    );
                } else {
                    println!(
                        "  {} Hook removed from {}",
                        "✓".green(),
                        process::to_display_path(&settings_path.to_string_lossy())
                    );
                }
            }
            Err(e) => {
                println!(
                    "  {} Could not serialize settings: {}",
                    "⚠".yellow(),
                    e
                );
            }
        }
    } else {
        println!(
            "  {} No terse hook found in Claude settings",
            "✓".green()
        );
    }
}

/// Check if a JSON value is a terse hook entry (either matcher-based or legacy flat).
fn is_terse_hook_entry(entry: &serde_json::Value) -> bool {
    // New matcher-based format: { "matcher": "Bash", "hooks": [{ "command": "...terse...hook..." }] }
    if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
        for hook in hooks {
            if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                if cmd.contains("terse") && cmd.contains("hook") {
                    return true;
                }
            }
        }
    }
    // Legacy flat format: { "type": "command", "command": "...terse...hook..." }
    if let Some(cmd) = entry.get("command").and_then(|c| c.as_str()) {
        if cmd.contains("terse") && cmd.contains("hook") {
            return true;
        }
    }
    false
}

/// Remove terse PATH entries from shell profiles (Unix) or user PATH (Windows).
fn uninstall_remove_path() {
    println!("{}", "Removing from PATH...".bold());

    let Some(bin_dir) = process::terse_bin_dir() else {
        println!("  {} Could not determine bin directory", "⚠".yellow());
        return;
    };

    let bin_str = bin_dir.to_string_lossy().to_string();

    #[cfg(not(target_os = "windows"))]
    {
        uninstall_remove_unix_path(&bin_str);
    }

    #[cfg(target_os = "windows")]
    {
        uninstall_remove_windows_path(&bin_str);
    }
}

/// Remove terse PATH entry from Unix shell profiles.
#[cfg(not(target_os = "windows"))]
fn uninstall_remove_unix_path(bin_dir: &str) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };

    let profiles = [
        home.join(".zshrc"),
        home.join(".bashrc"),
        home.join(".profile"),
        home.join(".bash_profile"),
        home.join(".config/fish/config.fish"),
    ];

    let mut removed_any = false;

    for profile in &profiles {
        if !profile.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(profile) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !content.contains(bin_dir) {
            continue;
        }

        // Filter out lines containing the terse bin dir or the terse comment
        let filtered: Vec<&str> = content
            .lines()
            .filter(|line| {
                !line.contains(bin_dir)
                    && !line.contains("# TERSE - Token Efficiency through Refined Stream Engineering")
            })
            .collect();

        // Trim trailing blank lines
        let mut result: Vec<&str> = filtered;
        while result.last().is_some_and(|l| l.trim().is_empty()) {
            result.pop();
        }

        let new_content = if result.is_empty() {
            String::new()
        } else {
            result.join("\n") + "\n"
        };

        if let Err(e) = std::fs::write(profile, &new_content) {
            println!(
                "  {} Could not update {}: {}",
                "⚠".yellow(),
                profile.display(),
                e
            );
        } else {
            println!(
                "  {} Removed PATH entry from {}",
                "✓".green(),
                process::to_display_path(&profile.to_string_lossy())
            );
            removed_any = true;
        }
    }

    if !removed_any {
        println!("  {} No PATH entry found in shell profiles", "✓".green());
    } else {
        println!(
            "  {} Restart your terminal for PATH changes to take effect",
            "⚠".yellow()
        );
    }
}

/// Remove terse bin directory from Windows user PATH.
#[cfg(target_os = "windows")]
fn uninstall_remove_windows_path(bin_dir: &str) {
    use std::process::Command;

    // Read current user PATH via PowerShell
    let output = match Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('PATH', 'User')",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            println!(
                "  {} Could not read user PATH: {}",
                "⚠".yellow(),
                e
            );
            return;
        }
    };

    let user_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if !user_path.contains(bin_dir) {
        println!("  {} Not found in PATH (nothing to remove)", "✓".green());
        return;
    }

    // Filter out our bin dir
    let new_path: String = user_path
        .split(';')
        .filter(|p| !p.is_empty() && *p != bin_dir)
        .collect::<Vec<_>>()
        .join(";");

    let set_cmd = format!(
        "[Environment]::SetEnvironmentVariable('PATH', '{}', 'User')",
        new_path
    );

    match Command::new("powershell")
        .args(["-NoProfile", "-Command", &set_cmd])
        .output()
    {
        Ok(o) if o.status.success() => {
            println!(
                "  {} Removed {} from user PATH",
                "✓".green(),
                bin_dir
            );
            println!(
                "  {} Restart your terminal for PATH changes to take effect",
                "⚠".yellow()
            );
        }
        _ => {
            println!(
                "  {} Could not update user PATH",
                "⚠".yellow()
            );
        }
    }
}

/// Remove terse files from disk.
///
/// On Unix, deleting a running binary is safe (inode stays alive until exit).
/// On Windows, the running exe is locked, so we spawn a detached `cmd /c`
/// process that waits for this process to exit, then deletes the directory.
fn uninstall_remove_files(keep_data: bool) {
    let Some(terse_home) = process::terse_home_dir() else {
        println!(
            "  {} Could not determine terse home directory",
            "⚠".yellow()
        );
        return;
    };

    if keep_data {
        println!("{}", "Removing binary (keeping config and data)...".bold());
        let bin_dir = terse_home.join("bin");
        if bin_dir.exists() {
            if try_remove_dir(&bin_dir) {
                println!("  {} Removed {}", "✓".green(), bin_dir.display());
            } else {
                schedule_windows_cleanup(&bin_dir);
            }
        } else {
            println!(
                "  {} Binary directory already removed",
                "✓".green()
            );
        }
        println!(
            "  {} Preserved data in {}",
            "✓".green(),
            process::to_display_path(&terse_home.to_string_lossy())
        );
    } else {
        println!("{}", "Removing all terse files...".bold());
        if terse_home.exists() {
            if try_remove_dir(&terse_home) {
                println!(
                    "  {} Removed {}",
                    "✓".green(),
                    process::to_display_path(&terse_home.to_string_lossy())
                );
            } else {
                schedule_windows_cleanup(&terse_home);
            }
        } else {
            println!(
                "  {} Terse home directory already removed",
                "✓".green()
            );
        }
    }
}

/// Try to remove a directory. Returns true on success.
/// On failure (e.g., Windows locked file), returns false.
fn try_remove_dir(path: &std::path::Path) -> bool {
    match std::fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(_) => false,
    }
}

/// On Windows, spawn a detached `cmd /c` that waits for our PID to exit,
/// then deletes the target directory. On Unix this is a no-op since
/// deletion of running binaries works natively.
#[allow(unused_variables)]
fn schedule_windows_cleanup(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let pid = std::process::id();
        let path_str = path.to_string_lossy();
        // Wait for our process to exit (tasklist loop), then rmdir /s /q.
        // START /b runs detached so we don't block.
        let cleanup_cmd = format!(
            "/c \"ping -n 2 127.0.0.1 >nul & \
             :retry & tasklist /fi \"PID eq {pid}\" 2>nul | find \"{pid}\" >nul && \
             (ping -n 1 127.0.0.1 >nul & goto retry) & \
             rmdir /s /q \"{path_str}\"\"",
        );
        let _ = std::process::Command::new("cmd")
            .args([&cleanup_cmd])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        println!(
            "  {} Scheduled cleanup of {} after exit",
            "✓".green(),
            path_str
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!(
            "  {} Could not remove {}: {}",
            "⚠".yellow(),
            path.display(),
            "unknown error"
        );
    }
}

// ---------------------------------------------------------------------------
// terse update
// ---------------------------------------------------------------------------

/// GitHub repo for release downloads.
const GITHUB_REPO: &str = "benwelker/terse";

/// Current binary version from Cargo.toml at compile time.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Update terse to the latest release from GitHub.
///
/// Downloads the appropriate binary for the current OS/architecture,
/// replaces the running binary, and reports success. On Windows, uses
/// the rename-to-`.old` technique since the running exe is locked.
pub fn run_self_update(force: bool) -> Result<()> {
    use std::io::Write;

    println!("{}", "terse Update".bold().cyan());
    println!("{}", "=".repeat(40));
    println!();
    println!("  Current version: {}", CURRENT_VERSION.bold());
    println!();

    // Step 1: Fetch latest release metadata
    println!("{}", "Checking for updates...".bold());
    let release_url = format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
    );
    let release: serde_json::Value = ureq::get(&release_url)
        .set("User-Agent", "terse-updater")
        .call()
        .context("could not fetch latest release from GitHub")?
        .into_json()
        .context("invalid release JSON from GitHub")?;

    let latest_tag = release["tag_name"]
        .as_str()
        .context("no tag_name in release")?;

    // Strip leading 'v' for comparison
    let latest_version = latest_tag.strip_prefix('v').unwrap_or(latest_tag);
    println!("  Latest version:  {}", latest_version.bold());

    if latest_version == CURRENT_VERSION {
        println!();
        println!("  {} Already up to date!", "✓".green());
        return Ok(());
    }

    println!();

    // Confirmation
    if !force {
        print!(
            "  Update {} → {}? [y/N] ",
            CURRENT_VERSION, latest_version
        );
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let answer = input.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!();
            println!("  {}", "Update cancelled.".yellow());
            return Ok(());
        }
        println!();
    }

    // Step 2: Find the correct asset URL
    let target = platform_asset_target();
    let assets = release["assets"]
        .as_array()
        .context("no assets in release")?;

    let download_url = assets
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?;
            if name.contains(&target) {
                a["browser_download_url"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .next()
        .with_context(|| format!("no release asset found for target: {target}"))?;

    println!("{}", format!("Downloading {target}...").bold());

    // Step 3: Download to a temp file
    let response = ureq::get(&download_url)
        .set("User-Agent", "terse-updater")
        .call()
        .context("download failed")?;

    let mut body = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body)
        .context("failed reading download")?;

    println!("  {} Downloaded {} bytes", "✓".green(), format_number(body.len()));

    // Step 4: Extract and replace binary
    println!("{}", "Installing update...".bold());
    let bin_dir = process::terse_bin_dir()
        .context("could not determine terse bin directory")?;
    let binary_name = process::terse_binary_name();
    let target_path = bin_dir.join(binary_name);

    std::fs::create_dir_all(&bin_dir)?;

    // The download is a .tar.gz on Unix, .zip on Windows.
    // Extract the binary from the archive.
    let extracted = extract_binary_from_archive(&body, binary_name)
        .context("failed to extract binary from archive")?;

    replace_binary(&target_path, &extracted)?;

    println!(
        "  {} Updated to {} at {}",
        "✓".green(),
        latest_version.bold(),
        process::to_display_path(&target_path.to_string_lossy())
    );

    // Step 5: Clean up stale .old file from previous Windows updates
    clean_old_binary(&bin_dir, binary_name);

    println!();
    println!("{}", "Update complete!".bold().cyan());
    println!();

    Ok(())
}

/// Determine the GitHub release asset target name for this platform.
fn platform_asset_target() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };

    format!("terse-{os}-{arch}")
}

/// Extract the terse binary from a downloaded archive.
///
/// Supports `.tar.gz` (Unix releases) and `.zip` (Windows releases).
/// Uses shell tools (`tar`, PowerShell `Expand-Archive`) to avoid extra
/// Rust dependencies. Falls back to treating the download as a raw binary
/// if extraction fails.
fn extract_binary_from_archive(
    data: &[u8],
    binary_name: &str,
) -> Result<Vec<u8>> {
    use std::io::Write;

    let temp_dir = std::env::temp_dir().join("terse-update");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)?;

    #[cfg(not(target_os = "windows"))]
    {
        let archive_path = temp_dir.join("download.tar.gz");
        let mut f = std::fs::File::create(&archive_path)?;
        f.write_all(data)?;
        drop(f);

        let status = std::process::Command::new("tar")
            .args(["xzf", &archive_path.to_string_lossy(), "-C", &temp_dir.to_string_lossy()])
            .status()
            .context("failed to run tar")?;

        if !status.success() {
            anyhow::bail!("tar extraction failed");
        }
    }

    #[cfg(target_os = "windows")]
    {
        let archive_path = temp_dir.join("download.zip");
        let mut f = std::fs::File::create(&archive_path)?;
        f.write_all(data)?;
        drop(f);

        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive_path.to_string_lossy(),
                    temp_dir.to_string_lossy()
                ),
            ])
            .status()
            .context("failed to run Expand-Archive")?;

        if !status.success() {
            anyhow::bail!("zip extraction failed");
        }
    }

    // Find the binary in the extracted contents (could be at root or nested)
    let binary_path = find_file_recursive(&temp_dir, binary_name)
        .with_context(|| format!("could not find {binary_name} in archive"))?;

    let content = std::fs::read(&binary_path)
        .context("failed to read extracted binary")?;

    // Cleanup temp
    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(content)
}

/// Recursively find a file by name in a directory.
fn find_file_recursive(
    dir: &std::path::Path,
    name: &str,
) -> Option<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = find_file_recursive(&path, name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Replace the binary at `target_path` with `new_binary`.
///
/// - Unix: write to a temp file in the same directory, then atomic rename.
/// - Windows: rename the running exe to `.old` (Windows allows rename of
///   locked files), then write the new binary.
fn replace_binary(
    target_path: &std::path::Path,
    new_binary: &[u8],
) -> Result<()> {
    let parent = target_path
        .parent()
        .context("binary path has no parent")?;

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;

        let temp_path = parent.join(".terse-update-tmp");
        std::fs::write(&temp_path, new_binary)
            .context("failed writing temp binary")?;

        // Set executable permission
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;

        // Atomic rename
        std::fs::rename(&temp_path, target_path)
            .context("failed to replace binary")?;
    }

    #[cfg(target_os = "windows")]
    {
        let old_path = parent.join(format!(
            "{}.old",
            target_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));

        // Remove previous .old if it exists (from a past update)
        let _ = std::fs::remove_file(&old_path);

        // Rename the running exe to .old (Windows allows this even while locked)
        if target_path.exists() {
            std::fs::rename(target_path, &old_path)
                .context("failed to rename running binary to .old")?;
        }

        // Write the new binary
        std::fs::write(target_path, new_binary)
            .context("failed writing new binary")?;
    }

    Ok(())
}

/// Clean up a stale `.old` binary from a previous Windows update.
/// Called at start or after a successful update.
fn clean_old_binary(bin_dir: &std::path::Path, binary_name: &str) {
    let old_path = bin_dir.join(format!("{binary_name}.old"));
    if old_path.exists() {
        let _ = std::fs::remove_file(&old_path);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(42), "42");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(12345), "12,345");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("ab", 2), "ab");
    }

    #[test]
    fn test_output_format_parsing() {
        assert_eq!(OutputFormat::from_str_opt(None), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str_opt(Some("json")), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str_opt(Some("csv")), OutputFormat::Csv);
        assert_eq!(
            OutputFormat::from_str_opt(Some("unknown")),
            OutputFormat::Table
        );
    }
}
