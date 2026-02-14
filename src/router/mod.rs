/// Router — central decision engine for TERSE's optimization pipeline.
///
/// The router provides two entry points:
///
/// - [`decide_hook`] — pre-execution gate for the hook (rewrite or passthrough)
/// - [`execute_run`] — post-execution pipeline for `terse run`
///
/// # Execution Model
///
/// ```text
/// ┌─────────────────┐     ┌─────────────────────────────────┐
/// │  terse hook      │     │  terse run                       │
/// │  (pre-execution) │     │  (post-execution)                │
/// │                  │     │                                  │
/// │  safety gates:   │     │  1. Run original command          │
/// │  - config        │     │  2. Preprocess output (always)   │
/// │  - loop guard    │────▶│  3. Size-based path decision     │
/// │  - heredoc       │     │  4. Fast path / Smart path / PT  │
/// │  - classifier    │     │                                  │
/// │  → Rewrite/Pass  │     └─────────────────────────────────┘
/// └─────────────────┘
/// ```
pub mod decision;

use anyhow::{Context, Result};

use crate::config;
use crate::config::schema::Mode;
use crate::llm;
use crate::llm::config::SmartPathConfig;
use crate::matching;
use crate::optimizers::OptimizerRegistry;
use crate::preprocessing;
use crate::safety::circuit_breaker::{CircuitBreaker, PathId};
use crate::safety::classifier::{self, CommandClass};
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

pub use decision::{HookDecision, OptimizationPath, PassthroughReason};

// ---------------------------------------------------------------------------
// Output size thresholds — loaded from config at runtime
// ---------------------------------------------------------------------------

/// Load output thresholds from config. Falls back to built-in defaults
/// if no config file is present.
fn load_thresholds() -> (usize, usize) {
    let cfg = config::load();
    (
        cfg.output_thresholds.passthrough_below_bytes,
        cfg.output_thresholds.smart_path_above_bytes,
    )
}

// ---------------------------------------------------------------------------
// Execution result
// ---------------------------------------------------------------------------

/// Result of executing a command through the router's optimization pipeline.
#[derive(Debug)]
pub struct ExecutionResult {
    /// The (possibly optimized) output text.
    pub output: String,
    /// Stderr content (populated only on passthrough).
    pub stderr: String,
    /// Which optimization path was taken.
    pub path: OptimizationPath,
    /// Token count of the original (raw) output.
    pub original_tokens: usize,
    /// Token count of the optimized output.
    pub optimized_tokens: usize,
    /// Name of the optimizer or path used (e.g. `"git"`, `"llm:llama3.2:1b"`, `"passthrough"`).
    pub optimizer_name: String,
    /// LLM latency in milliseconds (only populated for smart path).
    pub latency_ms: Option<u64>,
    /// Bytes removed by preprocessing.
    pub preprocessing_bytes_removed: Option<usize>,
    /// Percentage of bytes removed by preprocessing.
    pub preprocessing_pct: Option<f64>,
    /// Wall-clock time spent in the preprocessing pipeline (milliseconds).
    pub preprocessing_duration_ms: Option<u64>,
    /// Token count before preprocessing.
    pub preprocessing_tokens_before: Option<usize>,
    /// Token count after preprocessing.
    pub preprocessing_tokens_after: Option<usize>,
    /// Diagnostic: error from a higher-priority path that was attempted but
    /// failed before falling through to the current path.
    pub fallback_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Hook-level routing (safety gates only)
// ---------------------------------------------------------------------------

/// Make the pre-execution hook routing decision.
///
/// Called from the hook handler after parsing the request. Returns a
/// [`HookDecision`] indicating whether to rewrite the command to `terse run`
/// or pass through unchanged.
///
/// The hook only applies safety gates — the actual optimization path
/// (fast/smart/passthrough) is determined post-execution based on output
/// size after preprocessing.
///
/// # Decision Order
///
/// 1. Config gates: enabled, mode, safe_mode
/// 2. Loop guard: already a terse invocation → passthrough
/// 3. Heredoc: structurally complex → passthrough
/// 4. Classifier: destructive/editor command → passthrough
/// 5. Otherwise → rewrite to `terse run`
pub fn decide_hook(command: &str) -> HookDecision {
    // 0. Config gates
    let cfg = config::load();
    if !cfg.general.enabled {
        return HookDecision::Passthrough(PassthroughReason::NoPathAvailable);
    }
    if cfg.general.mode == Mode::Passthrough {
        return HookDecision::Passthrough(PassthroughReason::NoPathAvailable);
    }
    if cfg.general.safe_mode {
        return HookDecision::Passthrough(PassthroughReason::NoPathAvailable);
    }

    // 1. Loop guard
    if matching::is_terse_invocation(command) {
        return HookDecision::Passthrough(PassthroughReason::TerseInvocation);
    }

    // 2. Heredoc
    if matching::contains_heredoc(command) {
        return HookDecision::Passthrough(PassthroughReason::Heredoc);
    }

    // 3. Classifier
    if classifier::classify(command) == CommandClass::NeverOptimize {
        return HookDecision::Passthrough(PassthroughReason::NeverOptimize);
    }

    // 4. All safety gates passed — route through terse run.
    //    The actual path is decided post-execution based on output size.
    HookDecision::Rewrite
}

// ---------------------------------------------------------------------------
// Run-level execution
// ---------------------------------------------------------------------------

/// Execute a command through the full optimization pipeline.
///
/// Called from `terse run`. The pipeline is linear:
///
/// 1. **Run** the original command
/// 2. **Preprocess** the output (always — strip noise, dedup, truncate)
/// 3. **Size-based routing** using preprocessed output size:
///    - Below passthrough threshold → passthrough
///    - Above smart path threshold → smart path (preferred), fast path fallback
///    - Between thresholds → fast path
///    - Otherwise → passthrough
///
/// Records success/failure on the circuit breaker after each path attempt.
pub fn execute_run(command: &str) -> Result<ExecutionResult> {
    let cfg = config::load();
    let mut cb = CircuitBreaker::from_config(
        cfg.router.circuit_breaker_window,
        cfg.router.circuit_breaker_threshold,
        cfg.router.circuit_breaker_cooldown_secs,
    );
    let registry = OptimizerRegistry::new();
    let (passthrough_threshold, smart_path_threshold) = load_thresholds();
    let mode = &cfg.general.mode;

    let config_allows_optimization =
        cfg.general.enabled && *mode != Mode::Passthrough && !cfg.general.safe_mode;

    // --- Step 1: Run the original command ---
    let raw_output = run_shell_command(command).context("failed executing command in router")?;
    let raw_text = combine_stdout_stderr(&raw_output.stdout, &raw_output.stderr);
    let raw_bytes = raw_text.len();
    let raw_tokens = estimate_tokens(&raw_text);

    // --- Step 2: Preprocess output ---
    let preprocessed = preprocessing::preprocess(&raw_text, command);
    let pp_bytes_removed = preprocessed.bytes_removed;
    let pp_pct = preprocessed.reduction_pct;
    let pp_duration_ms = preprocessed.duration_ms;
    let pp_tokens_before = preprocessed.tokens_before;
    let pp_tokens_after = preprocessed.tokens_after;
    let output_bytes = preprocessed.text.len();

    // --- Step 3: Size-based path decision ---

    // Small output or config disables optimization → passthrough
    if output_bytes < passthrough_threshold || !config_allows_optimization {
        return Ok(ExecutionResult {
            original_tokens: raw_tokens,
            optimized_tokens: raw_tokens,
            path: OptimizationPath::Passthrough,
            optimizer_name: "passthrough".to_string(),
            output: raw_output.stdout,
            stderr: raw_output.stderr,
            latency_ms: None,
            preprocessing_bytes_removed: Some(pp_bytes_removed),
            preprocessing_pct: Some(pp_pct),
            preprocessing_duration_ms: Some(pp_duration_ms),
            preprocessing_tokens_before: Some(pp_tokens_before),
            preprocessing_tokens_after: Some(pp_tokens_after),
            fallback_reason: None,
        });
    }

    let smart_config = SmartPathConfig::load();
    let above_smart_threshold = output_bytes >= smart_path_threshold;

    // Track smart path failure for diagnostics
    let mut smart_path_error: Option<String> = None;

    // Smart path: LLM optimization (preferred for large outputs)
    //
    // When the output exceeds the smart-path threshold the LLM will
    // generally produce a better summary than rule-based optimizers, so
    // we attempt it first. The fast path serves as a fallback if the LLM
    // call fails.
    if above_smart_threshold
        && *mode != Mode::FastOnly
        && cb.is_allowed(PathId::SmartPath)
        && smart_config.enabled
    {
        match llm::optimize_with_llm(command, &preprocessed.text) {
            Ok(llm_result) => {
                cb.record_success(PathId::SmartPath);
                let output = append_truncation_footer(
                    &llm_result.output, raw_bytes,
                );
                return Ok(ExecutionResult {
                    original_tokens: raw_tokens,
                    optimized_tokens: llm_result.optimized_tokens,
                    path: OptimizationPath::SmartPath,
                    optimizer_name: format!("llm:{}", llm_result.model),
                    output,
                    stderr: String::new(),
                    latency_ms: Some(llm_result.latency_ms),
                    preprocessing_bytes_removed: Some(pp_bytes_removed),
                    preprocessing_pct: Some(pp_pct),
                    preprocessing_duration_ms: Some(pp_duration_ms),
                    preprocessing_tokens_before: Some(pp_tokens_before),
                    preprocessing_tokens_after: Some(pp_tokens_after),
                    fallback_reason: None,
                });
            }
            Err(err) => {
                let reason = format!("smart path failed: {err:#}");
                eprintln!("[terse] {reason}");
                smart_path_error = Some(reason);
                cb.record_failure(PathId::SmartPath);
                // Fall through to fast path as fallback.
            }
        }
    }

    // Fast path: rule-based optimizer
    //
    // Primary path for medium-sized outputs (between passthrough and smart
    // thresholds). Also serves as a fallback when the smart path is
    // unavailable or fails for large outputs.
    if *mode != Mode::SmartOnly
        && cfg.fast_path.enabled
        && cb.is_allowed(PathId::FastPath)
        && registry.can_handle(command)
    {
        match registry.optimize_first(command, &preprocessed.text) {
            Some(result) => {
                cb.record_success(PathId::FastPath);
                let output = append_truncation_footer(
                    &result.output, raw_bytes,
                );
                return Ok(ExecutionResult {
                    original_tokens: raw_tokens,
                    optimized_tokens: result.optimized_tokens,
                    path: OptimizationPath::FastPath,
                    optimizer_name: result.optimizer_used,
                    output,
                    stderr: String::new(),
                    latency_ms: None,
                    preprocessing_bytes_removed: Some(pp_bytes_removed),
                    preprocessing_pct: Some(pp_pct),
                    preprocessing_duration_ms: Some(pp_duration_ms),
                    preprocessing_tokens_before: Some(pp_tokens_before),
                    preprocessing_tokens_after: Some(pp_tokens_after),
                    fallback_reason: smart_path_error,
                });
            }
            None => {
                cb.record_failure(PathId::FastPath);
                // Fall through to passthrough.
            }
        }
    }

    // --- Passthrough (no optimizer matched, or paths failed/disabled) ---
    let output = append_truncation_footer(&raw_output.stdout, raw_bytes);
    Ok(ExecutionResult {
        original_tokens: raw_tokens,
        optimized_tokens: raw_tokens,
        path: OptimizationPath::Passthrough,
        optimizer_name: "passthrough".to_string(),
        output,
        stderr: raw_output.stderr,
        latency_ms: None,
        preprocessing_bytes_removed: Some(pp_bytes_removed),
        preprocessing_pct: Some(pp_pct),
        preprocessing_duration_ms: Some(pp_duration_ms),
        preprocessing_tokens_before: Some(pp_tokens_before),
        preprocessing_tokens_after: Some(pp_tokens_after),
        fallback_reason: smart_path_error,
    })
}

// ---------------------------------------------------------------------------
// Preview (for `terse test`)
// ---------------------------------------------------------------------------

/// Preview result for `terse test` — shows what the router would do.
#[derive(Debug)]
pub struct PreviewResult {
    /// Hook-level decision.
    pub hook_decision: String,
    /// Actual execution result (after running the command).
    pub execution: ExecutionResult,
}

/// Preview the optimization pipeline for a command.
///
/// Runs the hook decision logic (without actually hooking), then executes
/// the command through the router. Used by `terse test "command"`.
pub fn preview(command: &str) -> Result<PreviewResult> {
    let hook = decide_hook(command);
    let hook_desc = match &hook {
        HookDecision::Rewrite => "rewrite".to_string(),
        HookDecision::Passthrough(reason) => format!("passthrough ({reason})"),
    };

    let execution = execute_run(command)?;

    Ok(PreviewResult {
        hook_decision: hook_desc,
        execution,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Append a truncation footer to output when the final output is
/// significantly smaller than the raw command output. This is applied in the
/// router *after* the optimizer produces its final output so Claude knows
/// the output was reduced — regardless of whether preprocessing is enabled
/// or which optimization path was taken.
fn append_truncation_footer(output: &str, raw_bytes: usize) -> String {
    let output_bytes = output.len();
    if raw_bytes == 0 || output_bytes >= raw_bytes {
        return output.to_string();
    }
    let removed = raw_bytes - output_bytes;
    let mut pct = (removed as f64 / raw_bytes as f64) * 100.0;
    // Cap at 99.9% when output is non-empty — 100.0% would be misleading.
    if output_bytes > 0 && pct >= 100.0 {
        pct = 99.9;
    }
    format!(
        "{output}[output truncated: showing {output_bytes} of {raw_bytes} bytes ({pct:.2}% removed)]"
    )
}

/// Combine stdout and stderr into a single string for token counting.
fn combine_stdout_stderr(stdout: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    }
}
