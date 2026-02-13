//! Analytics reporter — aggregation, stats, discovery, and trend analysis.
//!
//! Reads the JSONL command log and provides:
//! - **Stats**: top commands by token savings, path distribution
//! - **Analyze**: time-windowed trend analysis
//! - **Discover**: find high-frequency unoptimized commands

use std::collections::HashMap;

use crate::analytics::logger::{self, CommandLogEntry};

// ---------------------------------------------------------------------------
// Aggregated stats
// ---------------------------------------------------------------------------

/// Summary statistics for `terse stats`.
#[derive(Debug)]
pub struct Stats {
    pub total_commands: usize,
    pub total_original_tokens: usize,
    pub total_optimized_tokens: usize,
    pub total_savings_pct: f64,
    pub path_distribution: PathDistribution,
    pub command_stats: Vec<CommandStat>,
}

/// Per-command-type aggregated statistics.
#[derive(Debug, Clone)]
pub struct CommandStat {
    pub command: String,
    pub count: usize,
    pub total_original_tokens: usize,
    pub total_optimized_tokens: usize,
    pub avg_savings_pct: f64,
    /// Most-used optimizer for this command type.
    pub primary_optimizer: String,
}

/// Distribution across optimization paths.
#[derive(Debug, Default)]
pub struct PathDistribution {
    pub fast: usize,
    pub smart: usize,
    pub passthrough: usize,
}

impl PathDistribution {
    /// Total number of commands across all paths.
    pub fn total(&self) -> usize {
        self.fast + self.smart + self.passthrough
    }

    /// Percentage for a given path, returns 0.0 if total is zero.
    pub fn pct(&self, count: usize) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            (count as f64 / total as f64) * 100.0
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery result
// ---------------------------------------------------------------------------

/// A candidate command identified by `terse discover` as worth building a
/// rule-based optimizer for (currently handled by passthrough or smart path).
#[derive(Debug, Clone)]
pub struct DiscoveryCandidate {
    pub command: String,
    pub count: usize,
    pub total_tokens: usize,
    pub avg_tokens: usize,
    pub current_path: String,
}

// ---------------------------------------------------------------------------
// Trend entry
// ---------------------------------------------------------------------------

/// A single data point in a time-based trend.
#[derive(Debug, Clone)]
pub struct TrendEntry {
    pub date: String,
    pub commands: usize,
    pub tokens_saved: usize,
    pub avg_savings_pct: f64,
}

// ---------------------------------------------------------------------------
// Stats computation
// ---------------------------------------------------------------------------

/// Compute aggregate stats from all log entries, optionally filtered to
/// the last `days` days.
pub fn compute_stats(days: Option<u32>) -> Stats {
    let entries = logger::read_entries_since_days(days);
    build_stats(&entries)
}

fn build_stats(entries: &[CommandLogEntry]) -> Stats {
    if entries.is_empty() {
        return Stats {
            total_commands: 0,
            total_original_tokens: 0,
            total_optimized_tokens: 0,
            total_savings_pct: 0.0,
            path_distribution: PathDistribution::default(),
            command_stats: Vec::new(),
        };
    }

    let total_commands = entries.len();
    let total_original_tokens: usize = entries.iter().map(|e| e.original_tokens).sum();
    let total_optimized_tokens: usize = entries.iter().map(|e| e.optimized_tokens).sum();

    let total_savings_pct = if total_original_tokens == 0 {
        0.0
    } else {
        let saved = total_original_tokens.saturating_sub(total_optimized_tokens);
        (saved as f64 / total_original_tokens as f64) * 100.0
    };

    let path_distribution = compute_path_distribution(entries);
    let command_stats = compute_command_stats(entries);

    Stats {
        total_commands,
        total_original_tokens,
        total_optimized_tokens,
        total_savings_pct,
        path_distribution,
        command_stats,
    }
}

fn compute_path_distribution(entries: &[CommandLogEntry]) -> PathDistribution {
    let mut dist = PathDistribution::default();
    for entry in entries {
        match entry.path.as_str() {
            "fast" => dist.fast += 1,
            "smart" => dist.smart += 1,
            _ => dist.passthrough += 1,
        }
    }
    dist
}

/// Group entries by base command name and compute per-command stats.
///
/// Returns sorted by total token savings (descending) — most impactful first.
fn compute_command_stats(entries: &[CommandLogEntry]) -> Vec<CommandStat> {
    let mut groups: HashMap<String, Vec<&CommandLogEntry>> = HashMap::new();
    for entry in entries {
        let base = logger::base_command_name(&entry.command).to_string();
        groups.entry(base).or_default().push(entry);
    }

    let mut stats: Vec<CommandStat> = groups
        .into_iter()
        .map(|(cmd, group)| {
            let count = group.len();
            let total_original: usize = group.iter().map(|e| e.original_tokens).sum();
            let total_optimized: usize = group.iter().map(|e| e.optimized_tokens).sum();

            let avg_savings = if count == 0 {
                0.0
            } else {
                group.iter().map(|e| e.savings_pct).sum::<f64>() / count as f64
            };

            // Find the most common optimizer
            let mut optimizer_counts: HashMap<&str, usize> = HashMap::new();
            for e in &group {
                *optimizer_counts.entry(&e.optimizer_used).or_default() += 1;
            }
            let primary_optimizer = optimizer_counts
                .into_iter()
                .max_by_key(|&(_, c)| c)
                .map(|(name, _)| name.to_string())
                .unwrap_or_default();

            CommandStat {
                command: cmd,
                count,
                total_original_tokens: total_original,
                total_optimized_tokens: total_optimized,
                avg_savings_pct: avg_savings,
                primary_optimizer,
            }
        })
        .collect();

    // Sort by total savings (original - optimized) descending
    stats.sort_by(|a, b| {
        let savings_a = a.total_original_tokens.saturating_sub(a.total_optimized_tokens);
        let savings_b = b.total_original_tokens.saturating_sub(b.total_optimized_tokens);
        savings_b.cmp(&savings_a)
    });

    stats
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Find high-frequency commands that are NOT handled by the fast path.
///
/// These are candidates for new rule-based optimizers. Returns sorted by
/// total token consumption (descending) — highest impact first.
pub fn discover_candidates(days: Option<u32>) -> Vec<DiscoveryCandidate> {
    let entries = logger::read_entries_since_days(days);

    // Only include commands that went through smart or passthrough paths
    let non_fast: Vec<&CommandLogEntry> = entries
        .iter()
        .filter(|e| e.path != "fast")
        .collect();

    let mut groups: HashMap<String, Vec<&CommandLogEntry>> = HashMap::new();
    for entry in &non_fast {
        let base = logger::base_command_name(&entry.command).to_string();
        groups.entry(base).or_default().push(entry);
    }

    let mut candidates: Vec<DiscoveryCandidate> = groups
        .into_iter()
        .map(|(cmd, group)| {
            let count = group.len();
            let total_tokens: usize = group.iter().map(|e| e.original_tokens).sum();
            let avg_tokens = if count == 0 { 0 } else { total_tokens / count };

            // Most common path for this command
            let mut path_counts: HashMap<&str, usize> = HashMap::new();
            for e in &group {
                *path_counts.entry(&e.path).or_default() += 1;
            }
            let current_path = path_counts
                .into_iter()
                .max_by_key(|&(_, c)| c)
                .map(|(p, _)| p.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            DiscoveryCandidate {
                command: cmd,
                count,
                total_tokens,
                avg_tokens,
                current_path,
            }
        })
        .collect();

    // Sort by total token consumption descending
    candidates.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    candidates
}

// ---------------------------------------------------------------------------
// Trends
// ---------------------------------------------------------------------------

/// Compute daily trend data over the last `days` days.
pub fn compute_trends(days: u32) -> Vec<TrendEntry> {
    let entries = logger::read_entries_since_days(Some(days));

    // Group by date (YYYY-MM-DD)
    let mut daily: HashMap<String, Vec<&CommandLogEntry>> = HashMap::new();
    for entry in &entries {
        // Parse date from RFC 3339 timestamp — take first 10 chars (YYYY-MM-DD)
        let date = entry.timestamp.get(..10).unwrap_or("unknown").to_string();
        daily.entry(date).or_default().push(entry);
    }

    let mut trends: Vec<TrendEntry> = daily
        .into_iter()
        .map(|(date, group)| {
            let commands = group.len();
            let tokens_saved: usize = group
                .iter()
                .map(|e| e.original_tokens.saturating_sub(e.optimized_tokens))
                .sum();
            let avg_savings_pct = if commands == 0 {
                0.0
            } else {
                group.iter().map(|e| e.savings_pct).sum::<f64>() / commands as f64
            };

            TrendEntry {
                date,
                commands,
                tokens_saved,
                avg_savings_pct,
            }
        })
        .collect();

    // Sort by date ascending
    trends.sort_by(|a, b| a.date.cmp(&b.date));

    trends
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<CommandLogEntry> {
        vec![
            CommandLogEntry {
                timestamp: "2025-01-15T10:00:00+00:00".to_string(),
                command: "git status".to_string(),
                path: "fast".to_string(),
                original_tokens: 500,
                optimized_tokens: 50,
                savings_pct: 90.0,
                optimizer_used: "git".to_string(),
                success: true,
                latency_ms: None,
            },
            CommandLogEntry {
                timestamp: "2025-01-15T10:05:00+00:00".to_string(),
                command: "git log --oneline".to_string(),
                path: "fast".to_string(),
                original_tokens: 1000,
                optimized_tokens: 200,
                savings_pct: 80.0,
                optimizer_used: "git".to_string(),
                success: true,
                latency_ms: None,
            },
            CommandLogEntry {
                timestamp: "2025-01-15T10:10:00+00:00".to_string(),
                command: "npm test".to_string(),
                path: "smart".to_string(),
                original_tokens: 2000,
                optimized_tokens: 600,
                savings_pct: 70.0,
                optimizer_used: "llm:llama3.2:1b".to_string(),
                success: true,
                latency_ms: Some(1500),
            },
            CommandLogEntry {
                timestamp: "2025-01-15T10:15:00+00:00".to_string(),
                command: "echo hello".to_string(),
                path: "passthrough".to_string(),
                original_tokens: 10,
                optimized_tokens: 10,
                savings_pct: 0.0,
                optimizer_used: "passthrough".to_string(),
                success: true,
                latency_ms: None,
            },
        ]
    }

    #[test]
    fn test_build_stats_totals() {
        let entries = sample_entries();
        let stats = build_stats(&entries);

        assert_eq!(stats.total_commands, 4);
        assert_eq!(stats.total_original_tokens, 3510);
        assert_eq!(stats.total_optimized_tokens, 860);
        assert!(stats.total_savings_pct > 75.0);
    }

    #[test]
    fn test_path_distribution() {
        let entries = sample_entries();
        let stats = build_stats(&entries);

        assert_eq!(stats.path_distribution.fast, 2);
        assert_eq!(stats.path_distribution.smart, 1);
        assert_eq!(stats.path_distribution.passthrough, 1);
    }

    #[test]
    fn test_command_stats_grouping() {
        let entries = sample_entries();
        let stats = build_stats(&entries);

        // git commands are grouped under "git"
        let git_stat = stats.command_stats.iter().find(|s| s.command == "git");
        assert!(git_stat.is_some());

        let git = git_stat.unwrap();
        assert_eq!(git.count, 2);
        assert_eq!(git.total_original_tokens, 1500);
        assert_eq!(git.primary_optimizer, "git");
    }

    #[test]
    fn test_empty_entries() {
        let stats = build_stats(&[]);
        assert_eq!(stats.total_commands, 0);
        assert_eq!(stats.total_savings_pct, 0.0);
    }

    #[test]
    fn test_discover_excludes_fast_path() {
        // Discovery should only find non-fast-path commands
        let entries = sample_entries();

        let non_fast: Vec<&CommandLogEntry> = entries
            .iter()
            .filter(|e| e.path != "fast")
            .collect();

        // npm test (smart) and echo (passthrough) should be candidates
        assert_eq!(non_fast.len(), 2);
    }

    #[test]
    fn test_trends_grouping() {
        let entries = sample_entries();

        // All entries are on the same date
        let mut daily: HashMap<String, Vec<&CommandLogEntry>> = HashMap::new();
        for entry in &entries {
            let date = entry.timestamp.get(..10).unwrap_or("unknown").to_string();
            daily.entry(date).or_default().push(entry);
        }

        assert_eq!(daily.len(), 1);
        assert!(daily.contains_key("2025-01-15"));
        assert_eq!(daily["2025-01-15"].len(), 4);
    }
}
