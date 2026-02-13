# TERSE — AI Agent Instructions

TERSE (Token Efficiency through Refined Stream Engineering) is a single Rust binary that intercepts AI coding assistant commands via hooks, optimizes outputs through a dual-path system (fast rule-based + LLM via Ollama), and reduces token consumption by 60–80%.

## Architecture

Dual-path execution: Claude Code PreToolUse hook → `terse hook` (stdin JSON → stdout JSON) rewrites commands to `terse run "<cmd>"` → router selects Fast Path (rule-based, <20ms), Smart Path (Ollama LLM, <2s), or Passthrough.

Key modules and their roles:

- `hook/` — reads hook JSON from stdin, returns passthrough `{}` or rewrite with `hookSpecificOutput.updatedInput`
- `router/` — central decision engine: `decide_hook()` (pre-execution) and `execute_run()` (post-execution)
- `optimizers/` — `Optimizer` trait + `OptimizerRegistry` (Vec<Box<dyn Optimizer>>), each optimizer uses command substitution or output post-processing
- `matching/` — `extract_core_command()` strips `cd`, env vars, subshells, `bash -c`, pipes before matching
- `llm/` — Ollama HTTP client (`ureq`, sync), category-aware prompts, output validation, feature flag config
- `safety/` — command classifier (NeverOptimize/Optimizable), per-path circuit breaker with file-backed state
- `analytics/` — JSONL command logging to `~/.terse/command-log.jsonl`
- `run/` — executor for `terse run` subcommand
- `cli/` — clap-derived subcommands

**Golden rule**: Any error → graceful passthrough (return raw output). Never break the Claude Code session.

## Build and Test

```bash
cargo build                          # Debug build
cargo build --release                # Release build
cargo test                           # All tests
cargo test -- --nocapture            # With stdout output
cargo clippy                         # Lint
cargo fmt --check                    # Format check
TERSE_TEST_LLM=1 cargo test llm_live # Live Ollama tests (requires running Ollama)
```

Uses **Rust edition 2024**. No async runtime — `ureq` for sync HTTP by design.

## Code Style

- **Error handling**: `anyhow::Result` throughout; `.context()` for error context; no `thiserror` yet
- **Best-effort I/O**: File writes (logs, circuit breaker) use `let _ =` to never fail the main operation
- **Module structure**: each module has `mod.rs` as entry with `pub mod` re-exports; `lib.rs` re-exports all modules; `main.rs` is minimal CLI dispatch
- **Section dividers**: `// ---------------------------------------------------------------------------` between logical sections
- **Naming**: RFC 430 — `PascalCase` types, `snake_case` functions, `SCREAMING_SNAKE_CASE` constants
- **Enums over flags**: `OptimizationPath`, `HookDecision`, `PassthroughReason`, `CommandClass`, `CommandCategory`
- **No async**: deliberately uses sync HTTP (`ureq`) — hook is inherently synchronous (stdin→stdout)
- **`unsafe`**: only for `std::env::set_var`/`remove_var` in tests (required in Rust 2024 edition)

## Project Conventions

**Optimizer pattern** — implement the `Optimizer` trait in `src/optimizers/mod.rs`:

```rust
pub trait Optimizer {
    fn name(&self) -> &'static str;
    fn can_handle(&self, ctx: &CommandContext) -> bool;
    fn execute_and_optimize(&self, ctx: &CommandContext) -> Result<OptimizedOutput>;
}
```

Register new optimizers in `OptimizerRegistry::new()`. Two strategies: command substitution (run a different command) or output post-processing (run original, transform result). See `src/optimizers/git.rs` for the reference implementation.

**CommandContext pattern** — `extract_core_command()` runs once, producing a `CommandContext` with `original` (full command) and `core` (extracted for matching). All optimizers receive this pre-extracted context.

**Hook protocol** — passthrough = `{}`, rewrite = `{ "hookSpecificOutput": { "updatedInput": { "command": "terse run ..." } } }` with `permissionDecision: "allow"`. Types in `src/hook/protocol.rs` use `#[serde(rename_all = "camelCase")]`.

**Config layering** — defaults → `~/.terse/config.json` → env vars (`TERSE_SMART_PATH`, `TERSE_SMART_PATH_MODEL`, etc.). Highest wins.

**Testing** — unit tests as `#[cfg(test)] mod tests` inside each source file; integration tests in `tests/` importing via `use terse::...`. Env-var-mutating tests combined into single `#[test]` to avoid race conditions.

## Implementation Status

Phases 1–4 complete (hook, git optimizer, matching engine, LLM smart path, router, circuit breaker). Phase 5 partial (logger done, CLI stubs). Phases 6–12 not started. See [TERSE-FINAL-Plan.md](.claude/plans/TERSE-FINAL-Plan.md) for the full roadmap.

## Runtime Files

- Config: `~/.terse/config.json`
- Logs: `~/.terse/command-log.jsonl`
- Circuit breaker: `~/.terse/circuit-breaker.json`
- Hook registration: `~/.claude/settings.json`
