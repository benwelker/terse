/// Router — central decision engine for TERSE's dual-path optimization.
///
/// The router encapsulates all routing logic that was previously spread across
/// the hook and run modules. It provides two entry points:
///
/// - [`decide_hook`] — pre-execution decision for the hook (rewrite or passthrough)
/// - [`execute_run`] — post-execution pipeline for `terse run` (fast → smart → passthrough)
///
/// Internally the router uses:
/// - [`classifier`](crate::safety::classifier) to reject destructive/editor commands
/// - [`CircuitBreaker`](crate::safety::circuit_breaker::CircuitBreaker) to disable
///   paths that are failing repeatedly
/// - [`OptimizerRegistry`](crate::optimizers::OptimizerRegistry) for rule-based fast path
/// - [`llm`](crate::llm) module for smart path
///
/// # Execution Model
///
/// The hook and run operate at different stages:
///
/// ```text
/// ┌─────────────────┐     ┌─────────────────────────────┐
/// │  terse hook      │     │  terse run                   │
/// │  (pre-execution) │     │  (post-execution)            │
/// │                  │     │                              │
/// │  classify cmd    │     │  1. Try fast path optimizer  │
/// │  check breakers  │     │  2. Run raw command          │
/// │  check optimizer │     │  3. Check output size        │
/// │  check smart     │────▶│  4. Try smart path (LLM)    │
/// │  → Rewrite/Pass  │     │  5. Fallback to passthrough  │
/// └─────────────────┘     └─────────────────────────────┘
/// ```
pub mod decision;

use anyhow::{Context, Result};

use crate::llm;
use crate::llm::config::SmartPathConfig;
use crate::matching;
use crate::optimizers::OptimizerRegistry;
use crate::safety::circuit_breaker::{CircuitBreaker, PathId};
use crate::safety::classifier::{self, CommandClass};
use crate::utils::process::run_shell_command;
use crate::utils::token_counter::estimate_tokens;

pub use decision::{HookDecision, OptimizationPath, PassthroughReason};

// ---------------------------------------------------------------------------
// Output size thresholds (bytes)
// ---------------------------------------------------------------------------

/// Outputs smaller than this are not worth optimizing — passthrough.
const PASSTHROUGH_THRESHOLD_BYTES: usize = 2 * 1024; // 2 KB

/// Outputs between `PASSTHROUGH_THRESHOLD_BYTES` and this value are eligible
/// for the fast path (rule-based optimizers only).
/// Outputs at or above this value are eligible for the smart path (LLM).
const SMART_PATH_THRESHOLD_BYTES: usize = 10 * 1024; // 10 KB

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
}

// ---------------------------------------------------------------------------
// Hook-level routing
// ---------------------------------------------------------------------------

/// Make the pre-execution hook routing decision.
///
/// Called from the hook handler after parsing the request. Returns a
/// [`HookDecision`] indicating whether to rewrite the command to `terse run`
/// or pass through unchanged.
///
/// # Decision Order
///
/// 1. Loop guard: already a terse invocation → passthrough
/// 2. Heredoc: structurally complex → passthrough
/// 3. Classifier: destructive/editor command → passthrough
/// 4. Circuit breaker + optimizer registry → fast path rewrite
/// 5. Circuit breaker + smart path availability → smart path rewrite
/// 6. Nothing available → passthrough
pub fn decide_hook(command: &str) -> HookDecision {
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

    let cb = CircuitBreaker::load();
    let registry = OptimizerRegistry::new();

    // 4. Fast path: rule-based optimizer available + circuit breaker open
    if cb.is_allowed(PathId::FastPath) && registry.can_handle(command) {
        return HookDecision::Rewrite {
            expected_path: OptimizationPath::FastPath,
        };
    }

    // 5. Smart path: feature enabled + Ollama healthy + circuit breaker open
    if cb.is_allowed(PathId::SmartPath) && llm::is_smart_path_available() {
        return HookDecision::Rewrite {
            expected_path: OptimizationPath::SmartPath,
        };
    }

    // 6. Nothing available
    HookDecision::Passthrough(PassthroughReason::NoPathAvailable)
}

// ---------------------------------------------------------------------------
// Run-level execution
// ---------------------------------------------------------------------------

/// Execute a command through the full optimization pipeline.
///
/// Called from `terse run`. The router tries each path in priority order:
///
/// 1. **Fast path** — rule-based optimizer (if circuit breaker allows)
/// 2. **Raw execution** — run the original command to capture output
/// 3. **Smart path** — LLM optimization (if output large enough + circuit breaker allows)
/// 4. **Passthrough** — return raw output unchanged
///
/// Records success/failure on the circuit breaker after each path attempt.
pub fn execute_run(command: &str) -> Result<ExecutionResult> {
    let mut cb = CircuitBreaker::load();
    let registry = OptimizerRegistry::new();

    // --- Fast path ---
    if cb.is_allowed(PathId::FastPath) && registry.can_handle(command) {
        match registry.execute_first(command) {
            Some(result) => {
                cb.record_success(PathId::FastPath);
                return Ok(ExecutionResult {
                    original_tokens: result.original_tokens,
                    optimized_tokens: result.optimized_tokens,
                    path: OptimizationPath::FastPath,
                    optimizer_name: result.optimizer_used,
                    output: result.output,
                    stderr: String::new(),
                    latency_ms: None,
                });
            }
            None => {
                // Optimizer matched (can_handle=true) but execution failed.
                cb.record_failure(PathId::FastPath);
            }
        }
    }

    // --- Run the original command raw ---
    let raw_output = run_shell_command(command)
        .context("failed executing command in router")?;
    let raw_text = combine_stdout_stderr(&raw_output.stdout, &raw_output.stderr);
    let raw_tokens = estimate_tokens(&raw_text);
    let output_bytes = raw_text.len();

    // --- Size-based routing (byte thresholds) ---
    //
    // < 2 KB   → passthrough (not worth optimizing)
    // 2–10 KB  → fast path eligible (rule-based post-processing only)
    // ≥ 10 KB  → smart path eligible (LLM)
    //
    // Note: command-substitution fast path already ran above (pre-execution).
    // The size gates below apply to *output post-processing* paths.

    if output_bytes < PASSTHROUGH_THRESHOLD_BYTES {
        return Ok(ExecutionResult {
            original_tokens: raw_tokens,
            optimized_tokens: raw_tokens,
            path: OptimizationPath::Passthrough,
            optimizer_name: "passthrough".to_string(),
            output: raw_output.stdout,
            stderr: raw_output.stderr,
            latency_ms: None,
        });
    }

    // --- Smart path (≥ 10 KB) ---
    let smart_config = SmartPathConfig::load();
    if output_bytes >= SMART_PATH_THRESHOLD_BYTES
        && cb.is_allowed(PathId::SmartPath)
        && smart_config.enabled
    {
        match llm::optimize_with_llm(command, &raw_text) {
            Ok(llm_result) => {
                cb.record_success(PathId::SmartPath);
                return Ok(ExecutionResult {
                    original_tokens: llm_result.original_tokens,
                    optimized_tokens: llm_result.optimized_tokens,
                    path: OptimizationPath::SmartPath,
                    optimizer_name: format!("llm:{}", llm_result.model),
                    output: llm_result.output,
                    stderr: String::new(),
                    latency_ms: Some(llm_result.latency_ms),
                });
            }
            Err(_) => {
                cb.record_failure(PathId::SmartPath);
                // Fall through to passthrough.
            }
        }
    }

    // --- Passthrough (2–10 KB without smart path, or smart path failed) ---
    Ok(ExecutionResult {
        original_tokens: raw_tokens,
        optimized_tokens: raw_tokens,
        path: OptimizationPath::Passthrough,
        optimizer_name: "passthrough".to_string(),
        output: raw_output.stdout,
        stderr: raw_output.stderr,
        latency_ms: None,
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
    /// Expected optimization path from hook analysis.
    #[allow(dead_code)]
    pub expected_path: OptimizationPath,
    /// Actual execution result (after running the command).
    pub execution: ExecutionResult,
}

/// Preview the optimization pipeline for a command.
///
/// Runs the hook decision logic (without actually hooking), then executes
/// the command through the router. Used by `terse test "command"`.
pub fn preview(command: &str) -> Result<PreviewResult> {
    let hook = decide_hook(command);
    let (hook_desc, expected) = match &hook {
        HookDecision::Rewrite { expected_path } => {
            (format!("rewrite (expected: {expected_path})"), *expected_path)
        }
        HookDecision::Passthrough(reason) => {
            (format!("passthrough ({reason})"), OptimizationPath::Passthrough)
        }
    };

    let execution = execute_run(command)?;

    Ok(PreviewResult {
        hook_decision: hook_desc,
        expected_path: expected,
        execution,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
