# TERSE — AI Agent Instructions

TERSE (Token Efficiency through Refined Stream Engineering) is a single Rust binary that intercepts AI coding assistant commands via hooks, optimizes outputs through a dual-path system (fast rule-based + LLM via Ollama), and reduces token consumption by 60–80%.

## Architecture

Execution flow: Claude Code PreToolUse hook → `terse hook` (stdin JSON → stdout JSON) rewrites commands to `terse run "<cmd>"` → router runs command, preprocesses output, then selects Fast Path (rule-based), Smart Path (Ollama LLM), or Passthrough.

Key modules and their roles:

- `hook/` — reads hook JSON from stdin, returns passthrough `{}` or rewrite with `hookSpecificOutput.updatedInput`
- `router/` — central decision engine: `decide_hook()` (pre-execution) and `execute_run()` (post-execution pipeline)
- `optimizers/` — `Optimizer` trait + `OptimizerRegistry` (`git`, `file`, `build`, `docker`, `generic`) for output post-processing
- `matching/` — `extract_core_command()` strips `cd`, env vars, subshells, `bash -c`, pipes before matching
- `preprocessing/` — deterministic pipeline (noise removal, path filtering, dedup, truncation, trim) before routing/optimization
- `llm/` — Ollama HTTP client (`ureq`, sync), category-aware prompts, output validation, smart-path config
- `config/` — layered config merge: defaults → `~/.terse/config.toml` → `.terse.toml` → `TERSE_*`
- `safety/` — command classifier (NeverOptimize/Optimizable), per-path circuit breaker with file-backed state
- `analytics/` — JSONL command logging to `~/.terse/command-log.jsonl`
- `run/` — executor for `terse run` subcommand
- `cli/` — clap-derived analytics/diagnostic/config commands
- `web/` — embedded dashboard + JSON API (`terse web`)

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

Register new optimizers in `OptimizerRegistry::from_config()`. TERSE optimizers are output post-processors (router executes original command first, optimizer transforms output). See `src/optimizers/git.rs` for the reference implementation.

**CommandContext pattern** — `extract_core_command()` runs once, producing a `CommandContext` with `original` (full command) and `core` (extracted for matching). All optimizers receive this pre-extracted context.

**Hook protocol** — passthrough = `{}`, rewrite = `{ "hookSpecificOutput": { "updatedInput": { "command": "terse run ..." } } }` with `permissionDecision: "allow"`. Types in `src/hook/protocol.rs` use `#[serde(rename_all = "camelCase")]`.

**Router pipeline** — `execute_run()` always preprocesses output first, then routes by size thresholds + mode/config/circuit-breaker gates (smart preferred for large outputs, fast fallback, passthrough otherwise).

**Config layering** — defaults → `~/.terse/config.toml` → `.terse.toml` → env vars (`TERSE_MODE`, `TERSE_PROFILE`, `TERSE_SMART_PATH`, etc.). Highest wins.

**Testing** — unit tests as `#[cfg(test)] mod tests` inside each source file; integration tests in `tests/` importing via `use terse::...`. Env-var-mutating tests combined into single `#[test]` to avoid race conditions.

## Implementation Status

Implemented today: hook routing, matching engine, preprocessing pipeline, fast-path optimizer suite (`git`, `file`, `build`, `docker`, `generic`), smart path via Ollama, circuit breaker, analytics commands, TOML configuration, and embedded web dashboard.

Primary CLI surface: `hook`, `run`, `stats`, `analyze`, `discover`, `health`, `test`, `config show|init|set|reset`, `web`.

## Runtime Files

- Config: `~/.terse/config.toml`
- Project config: `.terse.toml`
- Legacy smart-path JSON fallback: `~/.terse/config.json`
- Command log: `~/.terse/command-log.jsonl`
- Event log: `~/.terse/events.jsonl`
- Hook diagnostic log: `~/.terse/hook.log`
- Circuit breaker: `~/.terse/circuit-breaker.json`
- Hook registration: `~/.claude/settings.json`
