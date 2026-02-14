//! CLI command implementations for TERSE analytics and diagnostics.
//!
//! Provides subcommand handlers for:
//! - `terse stats` — token savings summary, path distribution, top commands
//! - `terse analyze --days N` — time-based trend analysis
//! - `terse discover` — find high-frequency unoptimized commands
//! - `terse health` — check Ollama, config, hook registration
//! - `terse test "command"` — preview optimization pipeline
//! - `terse config show|init|set|reset` — configuration management

use anyhow::Result;
use colored::Colorize;

use crate::analytics::logger;
use crate::analytics::reporter::{self, DiscoveryCandidate, Stats, TrendEntry};
use crate::config;
use crate::llm::config::SmartPathConfig;
use crate::llm::ollama::OllamaClient;
use crate::router;
use crate::safety::circuit_breaker::CircuitBreaker;

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
    println!("{}", "TERSE Token Savings Report".bold().cyan());
    println!("{}", "=".repeat(60));
    println!();

    // Summary
    let saved = stats
        .total_original_tokens
        .saturating_sub(stats.total_optimized_tokens);
    println!("  {} {}", "Total commands:".bold(), stats.total_commands);
    println!("  {} {}", "Tokens saved:  ".bold(), format_number(saved));
    println!(
        "  {} {:.1}%",
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
                "  {:<20} {:>6} {:>12} {:>9.1}% {}",
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
            "{},{},{},{},{:.1},{}",
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
        format!("TERSE Trends — Last {} Days", days).bold().cyan()
    );
    println!("{}", "=".repeat(50));
    println!(
        "  {:<12} {:>8} {:>12} {:>10}",
        "Date", "Commands", "Saved", "Avg %"
    );
    println!("  {}", "-".repeat(48));

    for entry in trends {
        println!(
            "  {:<12} {:>8} {:>12} {:>9.1}%",
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
            "{},{},{},{:.1}",
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
    println!("{}", "TERSE Health Check".bold().cyan());
    println!("{}", "=".repeat(40));

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
        // 2. Ollama connectivity
        let client = OllamaClient::from_config(&smart_config);
        let ollama_ok = client.is_healthy();
        let ollama_detail = if ollama_ok {
            format!("reachable at {}", smart_config.ollama_url)
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

    // 6. Hook registration hint
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
    println!("{}", "Effective TERSE Configuration".bold().cyan());
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
        "Edit the file to customize TERSE behavior.".dimmed()
    );
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

    println!("{}", "TERSE Optimization Preview".bold().cyan());
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
        println!(
            "  {} {} bytes removed ({:.1}%)",
            "Preprocessing:".bold(),
            pp_bytes,
            pp_pct,
        );
    }

    let savings = if preview.execution.original_tokens == 0 {
        0.0
    } else {
        let saved = preview
            .execution
            .original_tokens
            .saturating_sub(preview.execution.optimized_tokens);
        (saved as f64 / preview.execution.original_tokens as f64) * 100.0
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
        preview.execution.original_tokens,
        preview.execution.optimized_tokens,
        savings_display,
    );

    if let Some(latency) = preview.execution.latency_ms {
        println!("  {} {}ms", "Latency:      ".bold(), latency);
    }

    if let Some(ref reason) = preview.execution.fallback_reason {
        println!(
            "  {} {}",
            "Fallback:     ".bold(),
            reason.yellow()
        );
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
