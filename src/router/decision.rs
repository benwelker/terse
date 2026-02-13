//! Router decision types and in-memory decision cache.
//!
//! Defines the core types used by the router to classify optimization
//! decisions and communicate them to the hook and run modules.

use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Optimization path
// ---------------------------------------------------------------------------

/// Optimization path selected by the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptimizationPath {
    /// Rule-based optimizer handles this command (<20ms).
    FastPath,
    /// LLM smart path handles this command (<2s warm).
    SmartPath,
    /// No optimization — output passes through unchanged.
    Passthrough,
}

impl fmt::Display for OptimizationPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FastPath => write!(f, "fast"),
            Self::SmartPath => write!(f, "smart"),
            Self::Passthrough => write!(f, "passthrough"),
        }
    }
}

// ---------------------------------------------------------------------------
// Passthrough reason
// ---------------------------------------------------------------------------

/// Diagnostic reason explaining why a command was routed to passthrough.
///
/// Used by `terse test` to provide visibility into routing decisions and
/// by hook logging for debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassthroughReason {
    /// Command is already a terse invocation (infinite loop guard).
    TerseInvocation,
    /// Command contains a heredoc.
    Heredoc,
    /// Command classified as destructive or interactive (never optimize).
    NeverOptimize,
    /// No optimizer matched and smart path is unavailable.
    NoPathAvailable,
    /// Circuit breaker tripped for all viable paths.
    #[allow(dead_code)]
    AllCircuitsBroken,
    /// Output is too small to justify optimization (run-level only).
    #[allow(dead_code)]
    OutputTooSmall,
}

impl fmt::Display for PassthroughReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TerseInvocation => write!(f, "terse invocation (loop guard)"),
            Self::Heredoc => write!(f, "contains heredoc"),
            Self::NeverOptimize => write!(f, "destructive or editor command"),
            Self::NoPathAvailable => write!(f, "no optimizer or smart path available"),
            Self::AllCircuitsBroken => write!(f, "circuit breaker tripped for all paths"),
            Self::OutputTooSmall => write!(f, "output too small to optimize"),
        }
    }
}

// ---------------------------------------------------------------------------
// Hook decision
// ---------------------------------------------------------------------------

/// Result of the pre-execution hook routing decision.
#[derive(Debug)]
pub enum HookDecision {
    /// Rewrite the command to route through `terse run`.
    ///
    /// `expected_path` is informational — the actual path is decided at run
    /// time based on output size and runtime checks.
    Rewrite {
        expected_path: OptimizationPath,
    },
    /// Pass through unchanged — return empty JSON to Claude Code.
    Passthrough(PassthroughReason),
}

// ---------------------------------------------------------------------------
// Decision cache
// ---------------------------------------------------------------------------

/// In-memory cache mapping command patterns to optimization paths.
///
/// Since each `terse` invocation is a separate process, this cache only
/// benefits within a single process lifetime. It is included to document
/// the design intent and to support future long-running modes (daemon,
/// batch). Cross-process caching (file-backed) can be added if profiling
/// shows the registry lookup is a bottleneck.
#[allow(dead_code)]
pub struct DecisionCache {
    entries: HashMap<String, CachedEntry>,
    ttl: Duration,
}

#[allow(dead_code)]
struct CachedEntry {
    path: OptimizationPath,
    cached_at: Instant,
}

/// Default decision cache TTL in seconds.
#[allow(dead_code)]
pub const DEFAULT_CACHE_TTL_SECS: u64 = 300;

#[allow(dead_code)]
impl DecisionCache {
    /// Create a new cache with the given TTL in seconds.
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Look up a cached decision for the given command pattern.
    ///
    /// Returns `None` if the entry is missing or expired.
    pub fn get(&self, pattern: &str) -> Option<OptimizationPath> {
        self.entries.get(pattern).and_then(|entry| {
            if entry.cached_at.elapsed() < self.ttl {
                Some(entry.path)
            } else {
                None
            }
        })
    }

    /// Cache a decision for the given command pattern.
    pub fn insert(&mut self, pattern: String, path: OptimizationPath) {
        self.entries.insert(
            pattern,
            CachedEntry {
                path,
                cached_at: Instant::now(),
            },
        );
    }
}

impl Default for DecisionCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimization_path_display() {
        assert_eq!(OptimizationPath::FastPath.to_string(), "fast");
        assert_eq!(OptimizationPath::SmartPath.to_string(), "smart");
        assert_eq!(OptimizationPath::Passthrough.to_string(), "passthrough");
    }

    #[test]
    fn passthrough_reason_display() {
        assert!(!PassthroughReason::TerseInvocation.to_string().is_empty());
        assert!(!PassthroughReason::NeverOptimize.to_string().is_empty());
    }

    #[test]
    fn cache_returns_none_for_missing_key() {
        let cache = DecisionCache::default();
        assert_eq!(cache.get("git status"), None);
    }

    #[test]
    fn cache_stores_and_retrieves_decisions() {
        let mut cache = DecisionCache::new(300);
        cache.insert("git".to_string(), OptimizationPath::FastPath);

        assert_eq!(cache.get("git"), Some(OptimizationPath::FastPath));
    }

    #[test]
    fn cache_entries_expire_after_ttl() {
        let mut cache = DecisionCache::new(0); // 0-second TTL
        cache.insert("git".to_string(), OptimizationPath::FastPath);

        // With a 0-second TTL, the entry expires immediately.
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert_eq!(cache.get("git"), None);
    }
}
