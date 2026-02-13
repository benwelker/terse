/// Per-path circuit breaker with file-backed state.
///
/// Tracks success/failure rates for the fast path and smart path
/// independently. If the failure rate in a rolling window exceeds a
/// threshold, the path is "tripped" (disabled) for a cooldown period,
/// after which it auto-resumes.
///
/// State is persisted to `~/.terse/circuit-breaker.json` so that the
/// circuit breaker survives across short-lived `terse` process invocations.
/// All file I/O is best-effort — failures are silently ignored so the
/// circuit breaker never blocks command execution.
///
/// # Configuration Defaults
///
/// | Parameter | Default | Description                             |
/// |-----------|---------|-----------------------------------------|
/// | Window    | 10      | Number of recent results to track       |
/// | Threshold | 0.2     | Failure rate to trip (20%)              |
/// | Cooldown  | 600s    | Seconds to keep a path disabled         |
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Rolling window size — number of recent results to track per path.
const DEFAULT_WINDOW: usize = 10;

/// Failure rate threshold to trip the breaker (0.0–1.0).
const DEFAULT_THRESHOLD: f64 = 0.2;

/// Cooldown duration in seconds after tripping.
const DEFAULT_COOLDOWN_SECS: i64 = 600;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Identifies an optimization path for circuit breaker tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathId {
    FastPath,
    SmartPath,
}

/// File-backed circuit breaker for optimization paths.
///
/// Load with [`CircuitBreaker::load`], query with [`is_allowed`](Self::is_allowed),
/// and record outcomes with [`record_success`](Self::record_success) /
/// [`record_failure`](Self::record_failure). State is automatically persisted
/// on each record call.
pub struct CircuitBreaker {
    state: BreakerState,
    window: usize,
    threshold: f64,
    cooldown_secs: i64,
}

impl CircuitBreaker {
    /// Load circuit breaker state from disk, or create a fresh instance if
    /// the state file is missing or unreadable.
    pub fn load() -> Self {
        let state = load_state().unwrap_or_default();
        Self {
            state,
            window: DEFAULT_WINDOW,
            threshold: DEFAULT_THRESHOLD,
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
        }
    }

    /// Check whether the given path is currently allowed.
    ///
    /// Returns `false` if the path is tripped and the cooldown has not
    /// expired. Returns `true` otherwise (including when the cooldown has
    /// expired — the path auto-resumes).
    pub fn is_allowed(&self, path: PathId) -> bool {
        let ps = self.path_state(path);
        match ps.tripped_until {
            Some(deadline) if Utc::now() < deadline => false,
            _ => true,
        }
    }

    /// Record a successful execution on the given path.
    pub fn record_success(&mut self, path: PathId) {
        self.record(path, true);
    }

    /// Record a failed execution on the given path.
    pub fn record_failure(&mut self, path: PathId) {
        self.record(path, false);
    }

    /// Return a snapshot of the current state for diagnostics.
    #[allow(dead_code)]
    pub fn status(&self, path: PathId) -> PathStatus {
        let ps = self.path_state(path);
        let failures = ps.results.iter().filter(|&&ok| !ok).count();
        let total = ps.results.len();
        PathStatus {
            allowed: self.is_allowed(path),
            tripped_until: ps.tripped_until,
            recent_failures: failures,
            recent_total: total,
        }
    }

    // -- Internal --

    fn record(&mut self, path: PathId, success: bool) {
        // Copy config values before mutable borrow of path state.
        let window = self.window;
        let threshold = self.threshold;
        let cooldown_secs = self.cooldown_secs;

        let ps = self.path_state_mut(path);

        // Auto-resume: if the cooldown has expired, clear state.
        if let Some(deadline) = ps.tripped_until {
            if Utc::now() >= deadline {
                ps.tripped_until = None;
                ps.results.clear();
            }
        }

        ps.results.push(success);

        // Trim the window.
        if ps.results.len() > window {
            let excess = ps.results.len() - window;
            ps.results.drain(..excess);
        }

        // Check if failure rate exceeds threshold.
        if ps.results.len() >= window {
            let failures = ps.results.iter().filter(|&&ok| !ok).count();
            let failure_rate = failures as f64 / ps.results.len() as f64;
            if failure_rate > threshold {
                ps.tripped_until =
                    Some(Utc::now() + chrono::Duration::seconds(cooldown_secs));
            }
        }

        // Best-effort persist.
        let _ = save_state(&self.state);
    }

    fn path_state(&self, path: PathId) -> &PathState {
        match path {
            PathId::FastPath => &self.state.fast_path,
            PathId::SmartPath => &self.state.smart_path,
        }
    }

    fn path_state_mut(&mut self, path: PathId) -> &mut PathState {
        match path {
            PathId::FastPath => &mut self.state.fast_path,
            PathId::SmartPath => &mut self.state.smart_path,
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Snapshot of a single path's circuit breaker state.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PathStatus {
    /// Whether the path is currently allowed.
    pub allowed: bool,
    /// If tripped, when the cooldown expires.
    pub tripped_until: Option<DateTime<Utc>>,
    /// Number of failures in the current window.
    pub recent_failures: usize,
    /// Total number of results in the current window.
    pub recent_total: usize,
}

// ---------------------------------------------------------------------------
// Serializable state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BreakerState {
    #[serde(default)]
    fast_path: PathState,
    #[serde(default)]
    smart_path: PathState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PathState {
    #[serde(default)]
    results: Vec<bool>,
    #[serde(default)]
    tripped_until: Option<DateTime<Utc>>,
}

impl Default for PathState {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            tripped_until: None,
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O (best-effort)
// ---------------------------------------------------------------------------

fn state_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".terse").join("circuit-breaker.json"))
}

fn load_state() -> Option<BreakerState> {
    let path = state_path()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_state(state: &BreakerState) -> Option<()> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let json = serde_json::to_string_pretty(state).ok()?;
    fs::write(path, json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_breaker_allows_all_paths() {
        let cb = CircuitBreaker {
            state: BreakerState::default(),
            window: DEFAULT_WINDOW,
            threshold: DEFAULT_THRESHOLD,
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
        };
        assert!(cb.is_allowed(PathId::FastPath));
        assert!(cb.is_allowed(PathId::SmartPath));
    }

    #[test]
    fn path_trips_after_exceeding_threshold() {
        let mut cb = CircuitBreaker {
            state: BreakerState::default(),
            window: 5,
            threshold: 0.4, // >40% failures in 5 → need 3+ failures
            cooldown_secs: 600,
        };

        // Record 3 failures and 2 successes (60% failure rate > 40%)
        cb.record(PathId::SmartPath, false);
        cb.record(PathId::SmartPath, false);
        cb.record(PathId::SmartPath, false);
        cb.record(PathId::SmartPath, true);
        cb.record(PathId::SmartPath, true);

        assert!(!cb.is_allowed(PathId::SmartPath));
        // Fast path unaffected
        assert!(cb.is_allowed(PathId::FastPath));
    }

    #[test]
    fn path_allows_below_threshold() {
        let mut cb = CircuitBreaker {
            state: BreakerState::default(),
            window: 10,
            threshold: 0.2,
            cooldown_secs: 600,
        };

        // 1 failure in 10 = 10% < 20% threshold
        for _ in 0..9 {
            cb.record(PathId::FastPath, true);
        }
        cb.record(PathId::FastPath, false);

        assert!(cb.is_allowed(PathId::FastPath));
    }

    #[test]
    fn tripped_path_resumes_after_expired_cooldown() {
        let mut cb = CircuitBreaker {
            state: BreakerState::default(),
            window: 5,
            threshold: 0.4,
            cooldown_secs: 600,
        };

        // Trip the breaker
        for _ in 0..5 {
            cb.record(PathId::SmartPath, false);
        }
        assert!(!cb.is_allowed(PathId::SmartPath));

        // Manually set the deadline to the past (simulating cooldown expiry)
        cb.path_state_mut(PathId::SmartPath).tripped_until =
            Some(Utc::now() - chrono::Duration::seconds(1));

        assert!(cb.is_allowed(PathId::SmartPath));
    }

    #[test]
    fn window_trims_old_results() {
        let mut cb = CircuitBreaker {
            state: BreakerState::default(),
            window: 3,
            threshold: 0.5,
            cooldown_secs: 600,
        };

        // Record 3 failures → trips
        cb.record(PathId::FastPath, false);
        cb.record(PathId::FastPath, false);
        cb.record(PathId::FastPath, false);
        assert!(!cb.is_allowed(PathId::FastPath));

        // Manually reset trip to test window sliding
        cb.path_state_mut(PathId::FastPath).tripped_until = None;
        cb.path_state_mut(PathId::FastPath).results.clear();

        // Record 3 successes → window is all successes now
        cb.record(PathId::FastPath, true);
        cb.record(PathId::FastPath, true);
        cb.record(PathId::FastPath, true);
        assert!(cb.is_allowed(PathId::FastPath));
    }

    #[test]
    fn status_reports_correct_counts() {
        let mut cb = CircuitBreaker {
            state: BreakerState::default(),
            window: 10,
            threshold: 0.5,
            cooldown_secs: 600,
        };

        cb.record(PathId::FastPath, true);
        cb.record(PathId::FastPath, false);
        cb.record(PathId::FastPath, true);

        let status = cb.status(PathId::FastPath);
        assert!(status.allowed);
        assert_eq!(status.recent_failures, 1);
        assert_eq!(status.recent_total, 3);
    }
}
