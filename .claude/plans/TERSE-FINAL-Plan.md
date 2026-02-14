# TERSE — Unified Implementation Plan

## Token Efficiency through Refined Stream Engineering

---

## Project Overview

**Name:** TERSE (Token Efficiency through Refined Stream Engineering)

**Purpose:** Single Rust binary that intercepts AI coding assistant commands, optimizes outputs via fast rule-based optimizers AND a local LLM smart path (Ollama HTTP API), and reduces token consumption by 60–80%.

**Architecture:** Single Rust binary with dual optimization paths. Rule-based fast path (<20ms) for known commands, LLM smart path (<2s via Ollama HTTP API) for unknown/complex commands, intelligent router selects automatically.

**Primary Target:** Claude Code (via hooks/plugins)

**Future Targets:** GitHub Copilot CLI, other AI coding assistants

**Platform Support:** Windows (Phase 1), macOS & Linux (Phase 10)

**Tech Stack:** Rust (performance + cross-platform + single binary distribution)

**LLM Integration:** Ollama HTTP API called directly from Rust via `ureq` (sync HTTP client) — core feature

---

## Strategic Approach

### Why This Architecture:

- **Performance:** Rule-based optimizers execute in <20ms for known commands
- **Universal coverage:** LLM smart path handles any unknown command intelligently
- **Simplicity:** One binary, one language, one artifact to distribute
- **Ollama HTTP API:** Rust calls `localhost:11434` directly via `ureq` (sync HTTP) — no async runtime needed
- **Beginner-friendly Rust:** Phases introduce Rust concepts incrementally
- **Dual-path design:** Speed of rule-based optimizers for common commands + intelligence of LLM for everything else

### Hook-First, Plugin Later:

- Manual hook registration for rapid iteration during development
- Plugin packaging after core functionality is proven (Phase 11)
- Minimal refactoring needed to wrap existing binary as plugin

---

## Core Architecture

### Dual-Path Execution Model

> **Protocol note (critical):** Claude Code hooks use the `hookSpecificOutput` protocol.
> A PreToolUse hook **cannot** block-and-substitute (i.e., run the command itself and return
> the output). Instead, it can **rewrite** the command via `updatedInput`, and Claude Code
> then executes the rewritten command. TERSE uses this to rewrite commands to
> `terse run "original_command"`, so Claude Code executes `terse run` which does the
> optimization internally and prints the optimized output to stdout.

```
Claude Code wants to execute a Bash command
    ↓
PreToolUse Hook → terse hook (reads JSON from stdin)
    ↓
Step 1 — Match Check (pre-execution, no command execution here):
  Command already a terse invocation?       → Passthrough (avoid infinite loop)
  Command is destructive (rm, mv, >, >>)?   → Passthrough immediately
  Command is an editor (code, vim, nano)?   → Passthrough immediately
  Rule-based optimizer available?           → Rewrite to terse run
  Ollama available (Phase 3+)?              → Rewrite to terse run
  None of the above?                        → Passthrough
    ↓
Return JSON to stdout:
  Passthrough → empty `{}`
  Rewrite → `{ "hookSpecificOutput": { "updatedInput": { "command": "terse run ..." } } }`
    ↓
Claude Code executes the (possibly rewritten) command
    ↓ (if rewritten to terse run)
terse run "original_command":
  Step 2 — Execute the command via optimizer
  Step 3 — Router Decision (based on actual output byte size):
    Output < 2 KB?                           → Passthrough (not worth optimizing)
    Rule-based optimizer available (2–10 KB)? → Fast Path (Rust, <20ms)
    Ollama available AND output ≥ 10 KB?      → Preprocess → Smart Path (LLM, <2s)
    None of the above?                        → Run raw and pass through
  Step 3.5 — Preprocessing (Smart Path only, Phase 5-1):
    Noise removal (ANSI, progress bars, boilerplate)
    Path filtering (node_modules, target/debug, __pycache__, etc.)
    Deduplication (repeated lines/blocks → count annotation)
    Truncation with context preservation (if still > 32 KB)
    Trim (whitespace normalization)
  Step 4 — Post-Processing:
    Whitespace cleanup (applied to all optimized output)
    ↓
  Print optimized output to stdout → Claude Code receives the result
```

> **Note on optimization strategies:** Fast-path optimizers may use two techniques:
>
> 1. **Command substitution** — run an optimized version of the command (e.g., `git status --short --branch` instead of `git status`). The optimizer runs its own command and returns the result.
> 2. **Output post-processing** — run the original command as-is and transform the output (e.g., truncating, filtering, reformatting).
>    Each optimizer chooses the appropriate strategy. The Smart Path (LLM) always uses output post-processing.

### Path Characteristics

**Fast Path (Rule-Based Optimizers):**

- Common commands: git status, ls, docker ps, npm test, dotnet build
- Predictable output structure, regex-based transformation
- Speed: <20ms overhead
- Token savings: 70–90%
- Deterministic, zero external dependencies

**Smart Path (Local LLM via Ollama):**

- Unknown commands, complex/verbose output, unusual errors
- Category-aware prompt templates with few-shot examples
- Speed: <2s warm, <5s cold start
- Token savings: 60–80%
- Requires Ollama running with a model pulled

**Passthrough:**

- Critical operations: file edits, destructive commands (rm, mv, >)
- Tiny outputs: < 2 KB (not worth optimizing)
- Failed optimizations: validation errors, timeouts
- Zero overhead

### Project Structure

```
terse/
├── src/
│   ├── main.rs               # Entry point, mode routing (hook vs CLI)
│   ├── lib.rs                 # Shared library re-exports
│   ├── hook/
│   │   ├── mod.rs             # Hook mode handler (stdin JSON → stdout JSON)
│   │   └── protocol.rs       # Claude Code / Copilot hook JSON protocol
│   ├── cli/
│   │   ├── mod.rs             # CLI mode handler (clap)
│   │   └── commands.rs        # Stats, analyze, test, config, health subcommands
│   ├── router/
│   │   ├── mod.rs             # Decision engine: fast vs smart vs passthrough
│   │   └── decision.rs        # Path selection logic + caching
│   ├── optimizers/
│   │   ├── mod.rs             # Optimizer registry & trait definition
│   │   ├── git.rs             # Git command optimizers
│   │   ├── file.rs            # File operation optimizers (ls, find, cat)
│   │   ├── build.rs           # Build/test tool optimizers
│   │   ├── docker.rs          # Container optimizers
│   │   └── generic.rs         # Whitespace/generic cleanup pass
│   ├── llm/
│   │   ├── mod.rs             # LLM client abstraction
│   │   ├── ollama.rs          # Ollama HTTP API client
│   │   ├── prompts.rs         # Prompt templates per command category
│   │   └── validation.rs      # Output validation (length, hallucination check)
│   ├── analytics/
│   │   ├── mod.rs             # Analytics engine
│   │   ├── logger.rs          # Command logging (JSONL)
│   │   └── reporter.rs        # Stats, reports, discovery
│   ├── preprocessing/
│   │   ├── mod.rs             # Pipeline orchestrator
│   │   ├── noise.rs           # ANSI codes, progress bars, boilerplate
│   │   ├── path_filter.rs     # Directory/path noise filtering
│   │   ├── dedup.rs           # Repetitive content deduplication
│   │   ├── truncation.rs      # Truncation with context preservation
│   │   └── trim.rs            # Final whitespace normalization
│   ├── config/
│   │   ├── mod.rs             # Configuration system
│   │   └── schema.rs          # Config schema + defaults + hierarchy
│   ├── safety/
│   │   ├── mod.rs             # Safety mechanisms
│   │   ├── circuit_breaker.rs # Circuit breaker pattern
│   │   └── classifier.rs      # Command classification (safe/critical/passthrough)
│   └── utils/
│       ├── mod.rs
│       ├── process.rs         # Cross-platform subprocess execution
│       └── token_counter.rs   # Token estimation (char-based heuristic)
├── tests/
│   ├── hook_tests.rs          # Hook protocol integration tests
│   ├── optimizer_tests.rs     # Per-optimizer unit tests
│   ├── llm_tests.rs           # LLM integration tests (requires Ollama)
│   └── router_tests.rs        # Router decision tests
├── .github/
│   └── workflows/
│       ├── ci.yml             # Build + test on PR
│       └── release.yml        # Cross-platform release builds
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE (MIT)
└── CHANGELOG.md
```

### Runtime File Locations

- Config: `~/.terse/config.toml`
- Logs: `~/.terse/command-log.jsonl`
- Binary: `~/.terse/bin/terse[.exe]`
- Claude settings: `~/.claude/settings.json`

---

## Development Phases

### Phase 1: Rust Foundation + Hook Passthrough (Week 1) ✅ COMPLETE

**Goal:** Working binary that hooks into Claude Code, intercepts commands, and passes them through unchanged.

**Rust concepts introduced:** Project setup, modules, serde JSON, stdin/stdout, `Result` types, basic error handling with `anyhow`.

> **Note:** `anyhow` is used for error handling from Phase 1 onward. Phase 9 later refines this with custom error types via `thiserror`.

**Deliverables:**

- [x] Initialize Rust project: `cargo init terse`
- [x] Add dependencies to `Cargo.toml`: `serde`, `serde_json`, `clap` (derive), `chrono`, `dirs`, `anyhow`
- [x] Implement `src/main.rs` — detect mode (hook vs CLI) based on subcommand: `terse hook` vs `terse stats`
- [x] Implement `src/hook/protocol.rs` — define the Claude Code hook JSON schema:
  - Input: `{ "tool_name": "Bash", "tool_input": { "command": "git status" } }`
  - Passthrough output: empty JSON `{}` — Claude Code proceeds unchanged
  - Rewrite output: `{ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "terse command rewrite", "updatedInput": { "command": "terse run \"git status\"" } } }` — Claude Code executes the rewritten command
  - **Key protocol constraints:**
    - Hooks **cannot** return command output directly — they can only allow, deny, or rewrite
    - `updatedInput` modifies the tool input before execution
    - Exit code 0 + JSON = structured control; exit code 2 = deny; empty output = passthrough
- [x] Implement `src/hook/mod.rs` — read JSON from stdin, check if optimizer matches, return rewrite or passthrough
- [x] Implement `src/run/mod.rs` — executor for `terse run "command"`: runs optimizer, logs analytics, prints result to stdout
- [x] Implement `src/main.rs` — CLI with subcommands: `terse hook` (PreToolUse handler), `terse run <command>` (optimizer executor), `terse stats` (analytics)
- [x] Register hook manually in `~/.claude/settings.json`:
  ```json
  {
    "hooks": {
      "PreToolUse": [
        {
          "matcher": "Bash",
          "hooks": [
            {
              "type": "command",
              "command": "%USERPROFILE%\\.terse\\bin\\terse.exe hook"
            }
          ]
        }
      ]
    }
  }
  ```
  During development, point to the build output: `C:\source\repos\terse\target\debug\terse.exe hook`
- [x] Test: run Claude Code session, verify commands execute normally and output is unchanged

**Success Criteria:**

- Binary compiles and runs
- Hook intercepts commands successfully
- Commands pass through unchanged
- Claude Code session is completely unaffected

---

### Phase 2: Git Optimizer — End-to-End Token Savings (Week 2–3) ✅ COMPLETE

**Goal:** Ship the first measurable optimization — git commands producing 60–90% fewer tokens.

**Rust concepts introduced:** Traits, enums, `match`, regex, module organization, `#[cfg(test)]`, unit tests.

> **Routing in this phase:** Before the full router exists (Phase 4), Phases 2–3 use simple direct routing: if a rule-based optimizer matches the command → use it; else if LLM is available (Phase 3+) → use LLM; else → passthrough. Phase 4 replaces this with the full decision engine.

**Deliverables:**

- [x] Define the `Optimizer` trait in `src/optimizers/mod.rs`:
  - `fn can_handle(&self, command: &str) -> bool`
  - `fn execute_and_optimize(&self, command: &str) -> Result<OptimizedOutput>` — optimizer runs the command (or a substitute) and returns optimized output
  - `OptimizedOutput` struct: `{ output: String, original_tokens: usize, optimized_tokens: usize, optimizer_used: String }`
- [x] Create optimizer registry — a `Vec<Box<dyn Optimizer>>` that tries each in order via `execute_first()`
- [x] Implement `src/optimizers/git.rs` with these sub-optimizers:
  - `git status` → **command substitution**: run `git status --short --branch` instead, return compact output
  - `git log` → **command substitution**: run `git log --oneline -n 20` instead, return compact format
  - `git diff` → **output post-processing**: run original command, reduce context lines, truncate large diffs with summary
  - `git push/pull/fetch/add/commit` → **output post-processing**: capture output, return 1-line success/failure confirmation
  - `git branch` → **output post-processing**: compact list, highlight current branch
- [x] Implement `src/utils/token_counter.rs` — heuristic: `chars / 4` for token estimation
- [x] Implement `src/utils/process.rs` — cross-platform command execution wrapper
- [x] Wire optimizers into hook flow: hook checks optimizer match → rewrites to `terse run` via `updatedInput` → `terse run` executes optimizer → prints optimized output
- [x] Add basic logging to `~/.terse/command-log.jsonl`:
  - Timestamp, command, original tokens, optimized tokens, savings %, optimizer used
- [x] Write unit tests: test each git optimizer against sample outputs, verify token reduction
- [x] Manual testing with Claude Code: verify Claude understands optimized git output

**Success Criteria:**

- `git status`, `git log`, `git diff` produce 60–90% fewer tokens
- Claude Code still understands the optimized output
- Token savings logged to JSONL file
- All unit tests pass

---

### Phase 2-1: Command Matching Engine (Week 2–3, alongside Phase 2) ✅ COMPLETE

**Goal:** Robust extraction and matching of tool-use commands from Claude's shell invocations, handling prefixed, chained, and wrapped commands that simple equality checks would miss.

**Problem Statement:** Claude Code often wraps commands with `cd`, `&&` chains, environment variable prefixes, or subshell invocations. For example:

- `cd /home/user/project && git status`
- `LANG=C git diff`
- `(cd /repo && git log --oneline)`
- `bash -c "git status"`

A naive `command.starts_with("git")` check misses all of these. The existing `normalized_git_command()` in `git.rs` handles the `cd ... &&` pattern, but a unified, extensible matching engine is needed as more optimizers are added.

**Reference:** [RTK-AI's rtk-rewrite.sh](https://github.com/rtk-ai/rtk/blob/master/hooks/rtk-rewrite.sh) solves this in a 200+ line shell script using `jq`, `grep -qE`, and `sed` chains with many elif branches. TERSE handles this directly in Rust for better performance, type safety, single-binary distribution, and no dependency on external tools.

**Rust concepts introduced:** String processing, iterators, `Option` chaining, regex (optional).

**Deliverables:**

- [x] Implement `src/matching/mod.rs` — command extraction and normalization engine:
  - `extract_core_command(raw: &str) -> &str` — strip common wrappers:
    - `cd <path> &&` prefix → extract everything after `&&`
    - `ENV=val` prefixes → strip environment variable assignments
    - `bash -c "..."` / `sh -c "..."` → extract inner command
    - Subshell wrappers `(...)` → unwrap
    - Pipeline chains: match only the first command in `cmd1 | cmd2`
  - `matches_command(raw: &str, target: &str) -> bool` — check if the extracted command starts with `target`
  - Returns the normalized command for the optimizer to use
- [x] Refactor `normalized_git_command()` in `git.rs` to use the shared extraction engine
- [x] Update `OptimizerRegistry::can_handle()` to use the matching engine
- [x] Add the `terse run` infinite-loop guard to the matching engine (skip commands containing `terse` + `run`)
- [x] Write comprehensive unit tests for edge cases:
  - Simple: `git status` → matches `git`
  - Prefixed: `cd /repo && git status` → matches `git`
  - Env vars: `PAGER=cat git log` → matches `git`
  - Subshell: `(cd /repo && git diff)` → matches `git`
  - Shell wrapper: `bash -c "git status"` → matches `git`
  - Pipeline: `git log | head -20` → matches `git`
  - Non-match: `echo "git status"` → does NOT match `git`
  - Already terse: `terse run "git status"` → does NOT match (loop guard)

**Success Criteria:**

- All Phase 2 git optimizers work with prefixed/chained commands
- No false positives on quoted strings (e.g., `echo "git status"` is NOT a git command)
- Extraction handles real-world Claude Code patterns observed in analytics
- Matching adds <1ms overhead
- Easily extensible for new optimizers (file, build, docker) in later phases

---

### Phase 3: LLM Smart Path — Ollama Integration (Week 3–4) ✅ COMPLETE

**Goal:** Any command without a rule-based optimizer gets intelligently optimized by a local LLM.

> **Week overlap note:** Phase 3 starts in Week 3 while Phase 2 may still have remaining polish/tests in Week 3. The LLM module (`src/llm/`) is independent of the optimizer module (`src/optimizers/`), so they can be worked on in parallel during the overlap week. By end of Week 4, both paths are operational.

**Rust concepts introduced:** HTTP requests (`ureq`), JSON construction, timeouts, `Option`/`Result` chaining.

**Deliverables:**

- [x] Add `ureq` (sync HTTP with JSON feature) to `Cargo.toml`
- [x] Implement **runtime feature flag** for the LLM Smart Path:
  - Default: **disabled** (opt-in). The smart path must be explicitly enabled.
  - **Environment variable**: `TERSE_SMART_PATH=1` (or `true`) to enable, `0`/`false`/unset to disable.
  - **JSON config file**: `~/.terse/config.json` with `{ "smart_path": { "enabled": true } }`.
  - **Precedence**: env var > JSON config > default (disabled).
  - Implement in `src/llm/config.rs` — `SmartPathConfig` struct with `load()` method.
  - All LLM code paths check `SmartPathConfig::load().enabled` before activating.
- [x] Implement `src/llm/ollama.rs`:
  - `POST http://localhost:11434/api/generate` with model, prompt, stream=false
  - Configurable model name (default: `llama3.2:1b`)
  - Timeout: 5s cold start, 3s warm
  - Health check: `GET http://localhost:11434/api/tags` to detect Ollama availability
  - Return `Result<String>` — LLM response text
- [x] Implement `src/llm/prompts.rs` — category-aware prompt templates:
  - **Version control**: "Condense this git output. Keep: branch, changes, conflicts. Remove: verbose messages."
  - **File operations**: "Condense this directory listing. Keep: paths, sizes. Remove: permissions, timestamps."
  - **Build/test**: "Condense this build output. Keep: errors, warnings, failures. Remove: passing tests, progress."
  - **Container tools**: "Condense this Docker output. Keep: running containers, status. Remove: verbose IDs."
  - **Logs**: "Condense these logs. Keep: errors, warnings, unique messages. Remove: duplicates, debug noise."
  - **Generic fallback**: "Condense this command output, preserving all critical information for an AI coding assistant."
  - Each prompt includes few-shot examples and token budget instruction
- [x] Implement `src/llm/validation.rs`:
  - Check LLM response is non-empty
  - Check response is shorter than original (sanity)
  - Check for common hallucination markers (fabricated paths, invented status)
  - If validation fails → fall back to raw output
- [x] Wire LLM into **both** hook and run flows (two-level routing per execution model):
  - **Hook level** (`src/hook/mod.rs`): After checking rule-based optimizers, also check if smart path is enabled AND Ollama is healthy. If so, rewrite to `terse run` even though no rule-based optimizer matched. The hook **cannot** check output size — it runs pre-execution.
  - **Run level** (`src/run/mod.rs`): After executing the command raw (when no optimizer matched), check output byte size. If output ≥ 10 KB AND smart path is enabled AND Ollama is available → send to LLM. If output < 2 KB → passthrough (not worth optimizing). Outputs 2–10 KB fall through to passthrough if no fast-path optimizer handled them.
  - This two-level split aligns with the Core Architecture execution model where the hook makes the pre-execution rewrite decision and `terse run` makes the post-execution optimization decision.
- [x] Log LLM path usage: command, latency, tokens saved, model used
- [x] Write integration tests (gated behind `#[cfg(feature = "llm-tests")]` or environment variable)

**Prerequisite:** Ollama installed with `ollama pull llama3.2:1b`

**Recommended Models:**

| Model           | Size  | Speed (CPU) | Speed (GPU) | Quality   | Best For            |
| --------------- | ----- | ----------- | ----------- | --------- | ------------------- |
| Llama 3.2 1B ⭐ | 1.3GB | 200–500ms   | 50–150ms    | Excellent | Default, most users |
| Qwen 2.5 0.5B   | 0.5GB | 100–300ms   | 30–100ms    | Very good | Ultra-low latency   |
| Phi-3-mini 3.8B | 2.2GB | 400–800ms   | 100–250ms   | Excellent | Quality-first       |
| Gemma 2 2B      | 1.6GB | 300–600ms   | 80–200ms    | Very good | Balanced            |

**Success Criteria:**

- Commands without rule-based optimizers get LLM optimization
- Latency <2s warm, <5s cold
- Token savings 60%+
- Validation catches bad LLM output and falls back safely
- Ollama health check detects availability gracefully

---

### Phase 4: Router & Decision Engine (Week 4–5) ✅ COMPLETE

**Goal:** Intelligent automatic routing between fast path, smart path, and passthrough.

**Rust concepts introduced:** Enums as decision types, builder pattern, basic caching with TTL.

**Deliverables:**

- [x] Implement `src/safety/classifier.rs` — command classification:
  - **Passthrough list** (never optimize): `rm`, `mv`, `code`, `vim`, `nano`, `>`, `>>`
  - **Always optimize**: read-only commands (`git status`, `ls`, `grep`, `cat`, `docker ps`, `dotnet build`, `npm build`)
  - **Configurable**: everything else
- [x] Implement `src/router/decision.rs`:
  - `enum OptimizationPath { FastPath, SmartPath, Passthrough }`
  - Decision logic:
    1. Command in passthrough list? → `Passthrough`
    2. Output < 100 chars? → `Passthrough` (not worth optimizing)
    3. Rule-based optimizer available? → `FastPath`
    4. Ollama available AND output > 200 chars? → `SmartPath`
    5. Else → `Passthrough`
  - Cache recent decisions (command pattern → path) with 5-minute TTL
- [x] Implement `src/router/mod.rs` — orchestrate the full flow:
  - Classify command → select path → execute optimizer → validate → return
  - Log path decision with every command
- [x] Implement `src/safety/circuit_breaker.rs`:
  - Track failure rate per path (not global — if LLM is down, fast path still works)
  - If >20% failures in last 10 commands → disable that path for 10 minutes
  - Auto-resume after cooldown
  - Log circuit breaker state changes
- [x] Update hook handler to use router instead of direct optimizer calls
- [x] Add `terse test "command"` CLI subcommand — show which path would be selected and preview optimized output
- [x] Write router decision tests with various command/output combinations

**Success Criteria:**

- Router correctly selects optimal path 95%+ of the time
- Fast path used for known commands, smart path for unknown, passthrough for small/critical
- Circuit breaker protects against cascading failures per path independently
- `terse test` provides useful preview for debugging

---

### Phase 5: Analytics & CLI (Week 5–6) ✅ COMPLETE

**Goal:** Data-driven visibility into what TERSE is doing and where to optimize next.

**Rust concepts introduced:** `HashMap`/`BTreeMap`, iterators, functional chaining, formatted output, `colored` crate.

**Deliverables:**

- [x] Implement `src/analytics/logger.rs` — structured JSONL logging:
  - Fields: timestamp, command, path_selected, optimizer_used, original_tokens, optimized_tokens, savings_pct, latency_ms, success
  - Log all tools/commands for research. Include tool/command name and tokens. This will be used to find tools/commands we can add as future enhancements.
- [x] Implement `src/analytics/reporter.rs` — aggregation and reporting:
  - Group by command type, calculate totals
  - Rank by token savings potential
  - Trend analysis (daily, weekly)
- [x] Implement CLI subcommands in `src/cli/mod.rs`:
  - `terse stats` — top commands by token usage, total savings, path distribution (fast/smart/passthrough %)
  - `terse analyze --days N` — time-based analysis with trends
  - `terse discover` — find high-frequency unoptimized commands (candidates for new rule-based optimizers)
  - `terse test "command"` — preview optimization (from Phase 4)
  - `terse health` — check Ollama status, model availability, hook registration
- [x] Add terminal table formatting with `colored` crate for readable output
- [x] Export options: `--format json` and `--format csv` for all analytics commands
- [x] Implement `src/analytics/events.rs` — raw hook event logger (`~/.terse/events.jsonl`) capturing every hook invocation for discovery

**Example Output:**

```bash
$ terse stats
╔════════════════════╦═══════╦═══════════╦══════════╗
║ Command            ║ Count ║ Tokens    ║ Savings  ║
╠════════════════════╬═══════╬═══════════╬══════════╣
║ git status         ║   142 ║     8,520 ║  87.3%   ║
║ git log            ║    89 ║    12,460 ║  91.2%   ║
║ npm test (LLM)     ║    34 ║     6,800 ║  64.1%   ║
║ docker ps (LLM)    ║    21 ║     2,100 ║  72.5%   ║
╚════════════════════╩═══════╩═══════════╩══════════╝
Path Distribution: Fast 62% | Smart 23% | Passthrough 15%
Total Tokens Saved: 29,880 (78.4% average)
```

**Success Criteria:**

- Clear visibility into token savings, path distribution, and optimization opportunities
- `terse discover` identifies the next best rule-based optimizer to build
- Export formats work for further analysis

---

### Phase 5-1: Smart Path Preprocessing Pipeline (Week 6, alongside Phase 6) ✅ COMPLETE

**Goal:** Reduce raw output size by 40–70% _before_ sending to the LLM, improving smart path quality, latency, and token efficiency. Rust-based preprocessing is deterministic, fast (<5ms), and runs as a pipeline stage between raw command execution and LLM optimization.

**Rationale:** Sending 50 KB of raw `cargo test` output to a 1B-parameter model wastes context window, increases latency, and may cause the LLM to miss critical information buried in noise. Preprocessing strips the obvious waste so the LLM can focus on intelligent condensation of the remaining signal.

**Rust concepts introduced:** Iterator adaptors, `HashSet` for deduplication, regex for pattern matching, string slicing with context preservation.

**Deliverables:**

- [x] Create `src/preprocessing/mod.rs` — pipeline orchestrator:
  - `preprocess(raw: &str, command: &str) -> PreprocessedOutput`
  - `PreprocessedOutput` struct: `{ text: String, original_bytes: usize, preprocessed_bytes: usize, stages_applied: Vec<&'static str> }`
  - Pipeline runs stages in order; each stage receives the output of the previous
  - Entire pipeline target: <5ms for 100 KB input

- [x] Implement `src/preprocessing/noise.rs` — universal noise removal:
  - Strip ANSI escape codes / color sequences (`\x1b[...m`, `\x1b[...K`, etc.)
  - Remove progress bars, spinners, and carriage-return overwrite lines (`\r` without `\n`)
  - Strip download/upload progress indicators (e.g., `Downloading... 45%`, `████░░░░ 50%`)
  - Remove blank-line runs (collapse 3+ consecutive blank lines → single blank line)
  - Strip trailing whitespace from every line
  - Remove common boilerplate lines:
    - npm: `added N packages in Ns`, `up to date, audited N packages`
    - cargo: `Compiling ...`, `Downloading ...` (non-error lines)
    - dotnet: `Build succeeded.`, `Time Elapsed ...`
    - pip: `Requirement already satisfied`, `Successfully installed ...`
  - Configurable: boilerplate patterns stored as a list for future user extension

- [x] Implement `src/preprocessing/path_filter.rs` — directory/path noise filtering:
  - Comprehensive list of directories safe to filter from output:
    - **JavaScript/Node:** `node_modules`, `dist`, `.next`, `.nuxt`, `.cache`, `coverage`, `.turbo`
    - **Rust:** `target/debug`, `target/release`, `target/.fingerprint`, `target/build`
    - **Python:** `__pycache__`, `.venv`, `venv`, `env`, `.eggs`, `*.egg-info`, `.tox`, `.mypy_cache`, `.pytest_cache`
    - **Java/JVM:** `build/classes`, `build/libs`, `.gradle`, `target/classes` (Maven)
    - **.NET:** `bin/Debug`, `bin/Release`, `obj/Debug`, `obj/Release`, `packages`
    - **General:** `.git/objects`, `.git/refs`, `.git/logs`, `.hg`, `.svn`, `vendor` (Go), `Pods` (iOS)
    - **IDE/Editor:** `.idea`, `.vscode`, `.vs`, `*.swp`, `*.swo`
    - **Build artifacts:** `build`, `out`, `output`, `.build`, `cmake-build-*`
  - Filter modes:
    - **Line filter**: remove lines where the path segment appears (e.g., lines containing `node_modules/`)
    - **Summary**: replace N filtered lines with `[filtered N lines matching node_modules/*, dist/*, ...]`
  - Default: summary mode (preserves awareness that content was removed)

- [x] Implement `src/preprocessing/dedup.rs` — repetitive content deduplication:
  - Detect and collapse repeated lines (e.g., 200 `PASS src/tests/...` lines → `[200× PASS] src/tests/...`)
  - Detect and collapse repeated blocks (e.g., same warning repeated across files)
  - Similarity threshold: exact match for lines, configurable for blocks (future)
  - Preserve first and last occurrence with count annotation
  - Handle numbered sequences (e.g., `test 1/200`, `test 2/200` → `[tests 1–200/200: all passing]`)

- [x] Implement `src/preprocessing/truncation.rs` — truncation with context preservation:
  - If preprocessed output still exceeds a max size (default: 32 KB), truncate intelligently:
    - Preserve first N lines (command header / summary) and last M lines (final result / totals)
    - Insert `[... truncated {X} lines ({Y} bytes) ...]` marker in the middle
    - For structured output (e.g., test results), prefer keeping failures over passes
  - Section-aware truncation (future): detect sections by headers/blank-line boundaries, keep section starts

- [x] Implement `src/preprocessing/trim.rs` — final whitespace normalization:
  - Trim leading/trailing whitespace from full output
  - Normalize line endings to `\n`
  - Collapse runs of 3+ blank lines to 1 blank line (reinforces noise removal)
  - Strip any remaining trailing whitespace per line

- [x] Wire preprocessing into the router's smart path in `src/router/mod.rs`:
  - Before calling `llm::optimize_with_llm()`, run `preprocess(raw_text, command)`
  - Pass `preprocessed.text` to LLM instead of `raw_text`
  - Log preprocessing stats: original bytes, preprocessed bytes, stages applied
  - Update `ExecutionResult` to optionally carry preprocessing metadata

- [x] Add analytics tracking for preprocessing effectiveness:
  - Log `preprocessing_bytes_removed` and `preprocessing_pct` in command-log.jsonl
  - Add preprocessing stats to `terse stats` output

- [x] Write unit tests:
  - Noise removal: ANSI codes, progress bars, boilerplate patterns
  - Path filtering: node_modules paths, target/debug paths, mixed output
  - Deduplication: repeated lines, repeated blocks, numbered sequences
  - Truncation: context preservation, marker insertion
  - Full pipeline: raw cargo test output → preprocessed → verify critical info preserved
  - Edge cases: empty input, single-line input, already-small input (no-op)

**Example — Before/After preprocessing of `cargo test` output (50 KB → 8 KB):**

```
BEFORE (50 KB):
   Compiling serde v1.0.200
   Compiling serde_json v1.0.117
   ... (40 more Compiling lines)
   Compiling terse v0.1.0
running 140 tests
test analytics::logger::tests::test_log_entry ... ok
test analytics::logger::tests::test_read_entries ... ok
... (138 more "ok" lines)
test result: ok. 140 passed; 0 failed; 0 ignored; 0 measured

AFTER (8 KB):
running 140 tests
[140× ok] analytics::logger::tests::test_log_entry ... (and 139 more)
test result: ok. 140 passed; 0 failed; 0 ignored; 0 measured
```

**Success Criteria:**

- Preprocessing reduces output by 40–70% on average for outputs ≥ 10 KB
- Pipeline executes in <5ms for 100 KB input
- No critical information lost (errors, warnings, failures preserved)
- LLM quality improves: shorter input → more focused condensation
- Smart path latency decreases due to smaller prompt size
- All preprocessing stages individually testable and configurable

---

### Phase 6: Configuration System (Week 6–7) ✅ COMPLETE

**Goal:** User control over all behavior without code changes.

**Rust concepts introduced:** TOML parsing (`toml` crate), config hierarchy, path handling, default values.

**Deliverables:**

- [x] Add `toml` crate to `Cargo.toml`
- [x] Implement `src/config/schema.rs` — full config schema with defaults:

  ```toml
  [general]
  enabled = true
  mode = "hybrid"  # hybrid | fast-only | smart-only | passthrough
  # profile = "balanced"  # fast | balanced | quality (optional preset)

  [fast_path]
  enabled = true
  timeout_ms = 100

  [fast_path.optimizers]
  git = true
  file = true
  build = true
  docker = true
  whitespace = true

  [smart_path]
  enabled = true
  model = "llama3.2:1b"
  temperature = 0.0
  max_latency_ms = 3000
  ollama_url = "http://localhost:11434"
  # runtime = "ollama"  # Reserved for Phase 12: future values "llama-cpp", "openai-compat"

  [output_thresholds]
  # Byte-based thresholds for path routing (measured via output.len())
  passthrough_below_bytes = 2048    # < 2 KB  → passthrough
  smart_path_above_bytes  = 10240   # ≥ 10 KB → smart path; 2–10 KB → fast path

  [preprocessing]
  enabled = true
  max_output_bytes = 32768          # Truncate to 32 KB after preprocessing
  noise_removal = true              # ANSI codes, progress bars, boilerplate
  path_filtering = true             # Filter known noisy directories
  path_filter_mode = "summary"      # "summary" (annotated) or "remove" (silent)
  deduplication = true              # Collapse repeated lines/blocks
  truncation = true                 # Truncate with context preservation
  # Additional boilerplate patterns (appended to built-in list)
  # extra_boilerplate = ["pattern1", "pattern2"]
  # Additional directories to filter (appended to built-in list)
  # extra_filtered_dirs = ["custom_build/", ".special_cache/"]

  [router]
  decision_cache_ttl_secs = 300
  circuit_breaker_threshold = 0.2
  circuit_breaker_window = 10
  circuit_breaker_cooldown_secs = 600

  [passthrough]
  commands = ["code", "vim", "nano", "rm", "mv"]

  [logging]
  enabled = true
  path = "~/.terse/command-log.jsonl"
  level = "info"

  [whitespace]
  enabled = true
  max_consecutive_newlines = 2
  normalize_tabs = true
  trim_trailing = true
  ```

- [x] Implement `src/config/mod.rs` — config hierarchy:
  - Built-in defaults → `~/.terse/config.toml` (user global) → `.terse.toml` (project local)
  - Later entries override earlier ones
- [x] Add CLI config commands:
  - `terse config show` — display effective config
  - `terse config init` — create default config file
  - `terse config set <key> <value>` — update a setting
  - `terse config reset` — reset to defaults
- [x] Add performance profiles as named presets:
  - **fast**: prefers fast path, smart path min 1000 chars, timeout 50ms
  - **balanced**: default settings
  - **quality**: prefers smart path, min 200 chars, higher LLM timeout
  - Activated via: `terse config set general.profile balanced`

**Success Criteria:**

- Users can customize all behavior via TOML config
- Project-level overrides work (`.terse.toml` in repo root)
- Config changes take effect immediately (no restart)
- Performance profiles cover common use cases

---

### Phase 7: Expanded Optimizers (Week 7–8) ✅ COMPLETE

**Goal:** Cover 80%+ of commands by frequency with rule-based fast path.

**Rust concepts introduced:** Code organization at scale, DRY refactoring, trait objects, advanced pattern matching.

**Strategy:** Use `terse discover` output from Phase 5 to prioritize which optimizers to build next.

**Deliverables:**

- [x] Implement `src/optimizers/file.rs`:
  - `ls`/`dir` → tree-like compact format, limit to N items
  - `find` → compact paths, group by directory
  - `cat`/`type` → truncate long files with line count summary
  - `wc` → single-line result
- [x] Implement `src/optimizers/build.rs`:
  - `npm test`/`cargo test`/`dotnet test` → show failures only, pass/fail summary
  - `npm install`/`cargo build`/`dotnet build` → success/fail + error details only
- [x] Implement `src/optimizers/docker.rs`:
  - `docker ps` → compact table (name, image, status, ports)
  - `docker images` → compact list (repo, tag, size)
  - `docker logs` → tail + error extraction
- [x] Implement `src/optimizers/generic.rs` — whitespace optimization pass:
  - Max 2 consecutive newlines
  - Normalize tabs to spaces
  - Remove trailing whitespace
  - Trim leading/trailing blank lines
  - Applied as post-processing after any optimizer (including LLM output)
- [x] Update optimizer registry to include all new modules
- [x] Write unit tests for each optimizer against sample outputs
- [x] Before/after token comparison documentation
- [x] Add documentation for how to add additional optimizers

**Success Criteria:**

- 80%+ of command frequency covered by fast path
- Average 60%+ savings per new optimizer
- All tests pass
- `terse discover` shows diminishing returns (most high-value commands covered)

---

### Phase 8: Prompt Engineering & LLM Quality (Week 8–9) ⏭️ SKIP FOR NOW

**Goal:** Maximize LLM smart path quality and minimize hallucination.

**Rust concepts introduced:** String templating, structured output parsing, enum-based command categorization.

**Deliverables:**

- [ ] Refine prompt templates based on real-world analytics data:
  - Analyze which commands hit the smart path most via `terse stats`
  - Create category-specific prompts with few-shot examples
  - Add self-verification instructions: "Check: is your output shorter? Did you preserve all errors/warnings?"
- [ ] Implement A/B prompt comparison:
  - `terse test --compare "command"` — run multiple prompt variants, show side-by-side
- [ ] Add structured output format option — prompt LLM for JSON when beneficial:
  - `{"summary": "...", "errors": [...], "warnings": [...], "details": "..."}`
- [ ] Improve validation:
  - Compare key tokens (file paths, error codes) between raw and LLM output
  - Detect when LLM invents content not in the original
  - Track validation failure rate in analytics
- [ ] Experiment with model sizes and document benchmarks:
  - `llama3.2:1b` (default, fast)
  - `qwen2.5:0.5b` (ultra-fast)
  - `phi3:mini` (higher quality)

**Success Criteria:**

- Hallucination rate <1%
- Smart path token savings 65%+
- Validation catches 95%+ of bad outputs
- Prompt benchmarks documented for model selection guidance

---

### Phase 9: Safety, Reliability & Error Handling Refinement (Week 9–10) ⏭️ NEXT

**Goal:** Bulletproof error handling — TERSE never breaks a Claude Code session.

**Rust concepts introduced:** Custom error types with `thiserror` crate, `Drop` trait for cleanup, structured error hierarchies.

> **Note:** Basic error handling with `anyhow` has been in use since Phase 1. This phase upgrades to a structured error taxonomy with `thiserror`, adds safety mechanisms (safe mode, rate limiting, sensitive data detection), and creates the comprehensive integration test suite.

**Deliverables:**

- [ ] Implement comprehensive error taxonomy with `thiserror`:
  - `HookError`, `OptimizerError`, `LlmError`, `ConfigError`, `RouterError`
  - All errors result in graceful passthrough (return raw output)
  - Replace ad-hoc `anyhow` usage in earlier phases with these typed errors where beneficial
- [ ] Implement safe mode:
  - Environment variable: `TERSE_SAFE_MODE=1`
  - Config: `general.safe_mode = true`
  - Disables all optimization, logs only
- [ ] Rate limiting for LLM path:
  - Max N concurrent LLM requests (default: 1)
  - Queue additional requests with timeout
- [ ] Sensitive data detection in `src/safety/classifier.rs`:
  - Skip optimization if output contains patterns: API keys, passwords, tokens
  - Configurable pattern list
- [ ] Automatic hook health monitoring:
  - Track consecutive failures
  - Log warnings to stderr (visible in Claude Code)
  - Circuit breaker disables paths, not the entire hook
- [ ] Integration test suite — end-to-end scenarios:
  - Happy path (optimizer succeeds)
  - LLM timeout (falls back to raw)
  - LLM hallucination (validation catches, falls back)
  - Ollama not running (skips smart path, fast path still works)
  - Malformed input (returns passthrough)
  - Very large output (truncation handling)
- [ ] Edge case tests: binary output, unicode, empty output, extremely long commands

**Success Criteria:**

- Zero Claude Code session disruptions
- All failures result in usable output (graceful passthrough)
- Circuit breaker auto-recovers
- <0.1% hook failure rate
- Safe mode provides escape hatch

---

### Phase 10: Cross-Platform & CI/CD (Week 10–11) ✅ COMPLETE

**Goal:** Build and distribute for Windows, macOS, and Linux.

**Rust concepts introduced:** Conditional compilation (`#[cfg]`), platform abstractions, GitHub Actions matrix builds.

**Deliverables:**

- [x] Platform-specific command handling in `src/utils/process.rs`:
  - Windows `dir` ↔ Unix `ls`
  - Windows `type` ↔ Unix `cat`
  - Path separator normalization
- [x] Platform-specific optimizer logic using `#[cfg(target_os = "...")]` where needed
- [x] GitHub Actions CI workflow (`.github/workflows/ci.yml`):
  - Run `cargo test`, `cargo clippy`, `cargo fmt --check` on PR
  - Matrix: windows-latest, macos-latest, ubuntu-latest
- [x] GitHub Actions release workflow (`.github/workflows/release.yml`):
  - Trigger on tag push (`v*`)
  - Build matrix:
    - `x86_64-pc-windows-msvc` (Windows x64)
    - `x86_64-apple-darwin` (macOS x64)
    - `aarch64-apple-darwin` (macOS ARM64)
    - `x86_64-unknown-linux-gnu` (Linux x64)
  - Upload binaries as release assets
- [x] Installation scripts:
  - `install.ps1` (Windows PowerShell)
  - `install.sh` (macOS/Linux)
  - Both: download binary → place in `~/.terse/bin/` → create default config → check Ollama → register hook
- [?] Platform-specific test suite

**Success Criteria:**

- Single codebase produces working binaries for all three platforms
- CI tests pass on all platforms
- One-command install on each OS
- Installation scripts handle Ollama detection and hook registration

---

### Phase 11: Claude Code Plugin Packaging (Week 11–12)

**Goal:** Distribute as an official Claude Code plugin for one-command install.

**Deliverables:**

- [ ] Create `.claude-plugin/plugin.json` manifest:
  ```json
  {
    "name": "terse",
    "version": "1.0.0",
    "description": "Token Efficiency through Refined Stream Engineering - Reduce token usage by 60-80%",
    "author": "Benjamin Welker",
    "license": "MIT",
    "hooks": {
      "PreToolUse": [
        {
          "matcher": "Bash",
          "hooks": [
            {
              "type": "command",
              "command": "${PLUGIN_DIR}/bin/terse${EXE_SUFFIX} hook",
              "platforms": ["win32", "darwin", "linux"]
            }
          ]
        }
      ]
    },
    "commands": [
      {
        "name": "stats",
        "description": "Show token savings statistics",
        "script": "${PLUGIN_DIR}/bin/terse${EXE_SUFFIX} stats"
      },
      {
        "name": "analyze",
        "description": "Analyze usage patterns",
        "script": "${PLUGIN_DIR}/bin/terse${EXE_SUFFIX} analyze"
      },
      {
        "name": "health",
        "description": "Check system health",
        "script": "${PLUGIN_DIR}/bin/terse${EXE_SUFFIX} health"
      }
    ],
    "setup": {
      "script": "${PLUGIN_DIR}/setup.sh",
      "description": "Downloads LLM model and validates installation"
    }
  }
  ```
- [ ] Bundle pre-built binaries per platform in `bin/` directory
- [ ] Add setup script: check Ollama, download model, validate installation
- [ ] Test with `claude --plugin-dir ./terse`
- [ ] Publish to personal GitHub plugin marketplace
- [ ] Submit to community registries
- [ ] Comprehensive README: features, installation, configuration reference, troubleshooting

**Success Criteria:**

- `claude plugin install terse` works
- No manual settings.json editing needed
- Setup handles Ollama model download
- Plugin appears in Claude Code, commands accessible via `/terse:stats`

---

### Phase 12: Copilot CLI Support & Advanced Features (Week 12+)

**Goal:** Expand to other AI coding assistants and add advanced capabilities.

**Deliverables:**

- [ ] **Copilot CLI investigation** — research hook/extension points, add protocol adapter if different from Claude Code
- [ ] **Context-aware routing** — detect debugging vs exploration vs refactoring from recent command history; adjust optimization aggressiveness:
  - Debugging: preserve more detail, lower smart path threshold
  - Exploration: aggressive optimization, prefer fast path
  - Refactoring: balanced approach
- [ ] **Adaptive learning** — track which path produces better results per command, adjust routing decisions over time
- [ ] **Model flexibility** — support additional LLM backends beyond Ollama:
  - `llama-cpp` (llama.cpp HTTP server — alternative self-hosted LLM server)
  - `openai-compat` (OpenAI-compatible APIs: LM Studio, LocalAI, etc.)
  - Configuration: `smart_path.runtime = "ollama" | "llama-cpp" | "openai-compat"`
- [ ] **Team config sharing** — project-level `.terse.toml` committed to repo for shared settings
- [ ] **Web dashboard** — optional local web UI for analytics visualization
- [ ] **Multi-step optimization** — use tool-calling models to execute follow-up commands and combine outputs
- [ ] **Fallback chaining** — try fast path → if fails, try smart path → if fails, passthrough (maximizes success rate)

---

## Success Metrics

### Technical Metrics

| Metric                       | Target                                 |
| ---------------------------- | -------------------------------------- |
| Token Reduction              | 60–80% average                         |
| Fast Path Latency            | <20ms average                          |
| Smart Path Latency (warm)    | <2s average                            |
| Smart Path Latency (cold)    | <5s                                    |
| Overall Average Latency      | <500ms (weighted by path distribution) |
| Command Coverage (fast path) | 80%+ by frequency                      |
| Hook Failure Rate            | <0.1%                                  |
| LLM Hallucination Rate       | <1%                                    |
| Validation Pass Rate         | 97%+                                   |

### Path Distribution (Target)

| Path        | % of Commands |
| ----------- | ------------- |
| Fast Path   | 60%           |
| Smart Path  | 20%           |
| Passthrough | 20%           |

### User Experience Metrics

| Metric            | Target                                                  |
| ----------------- | ------------------------------------------------------- |
| Installation Time | <5 minutes (plugin), <10 minutes (manual + Ollama)      |
| Configuration     | Zero-config works well out of the box                   |
| Learning Curve    | <30 minutes to understand                               |
| Debugging         | `terse health` + `terse test` provide clear diagnostics |

---

## Technology Stack

### Cargo.toml Dependencies

> **Note:** This is the _final_ `Cargo.toml` after all phases. Dependencies are added incrementally as each phase requires them. Phase 1 starts with only `serde`, `serde_json`, `clap`, `chrono`, `dirs`, and `anyhow`. Each phase's deliverables note which new dependencies to add.

```toml
[package]
name = "terse"
version = "0.1.0"
edition = "2021"
authors = ["Benjamin Welker"]
description = "Token Efficiency through Refined Stream Engineering"
license = "MIT"
repository = "https://github.com/benwelker/terse"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
regex = "1.10"
chrono = { version = "0.4", features = ["serde"] }
dirs = "5.0"
shellexpand = "3.1"
clap = { version = "4.5", features = ["derive"] }
colored = "2.1"
ureq = { version = "2.9", features = ["json"] }
thiserror = "1.0"
anyhow = "1.0"

[dev-dependencies]
assert_cmd = "2.0"
predicates = "3.1"
tempfile = "3.8"

[[bin]]
name = "terse"
path = "src/main.rs"
```

### Optional Dependencies (added as needed)

- `rusqlite` — upgrade from JSONL to SQLite for analytics
- `prettytable-rs` — terminal table formatting
- `indicatif` — progress bars for long operations
- `tokio` — async runtime (only if needed for advanced features)

---

## Key Architectural Decisions

| Decision                                       | Rationale                                                                                                   |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **Single Rust binary**                         | One artifact to distribute, Ollama HTTP API callable from Rust directly, no additional runtime dependencies |
| **TOML config** (not JSON)                     | Human-friendly, supports comments, standard in Rust ecosystem                                               |
| **Git optimizer first** (not analytics-first)  | Delivers measurable value immediately; analytics added Phase 5 to guide subsequent priorities               |
| **LLM as core feature**                        | Universal command coverage from Phase 3 onward — TERSE handles _any_ command                                |
| **`ureq` sync HTTP** (not `reqwest` async)     | Simpler for beginner Rust, hook is inherently synchronous (stdin→stdout), no async runtime needed           |
| **Circuit breaker per path** (not global)      | If LLM is down, fast path still works; if optimizer has bug, LLM fallback still works                       |
| **Whitespace optimizer as post-processing**    | Applied after both fast and smart paths for consistent additional savings                                   |
| **Preprocessing before LLM** (not raw input)   | 40–70% noise reduction before LLM improves quality, reduces latency, and saves LLM context window           |
| **Hook-first, plugin later**                   | Faster iteration during development; plugin packaging after core is proven                                  |
| **`anyhow` from Phase 1, `thiserror` Phase 9** | Start with simple error handling, refine to typed errors once the error landscape is understood             |

---

## Risk Management

### Technical Risks

| Risk                          | Mitigation                                                    | Fallback                                           |
| ----------------------------- | ------------------------------------------------------------- | -------------------------------------------------- |
| Hook breaks Claude Code       | Graceful passthrough on all errors; never fail silently       | Safe mode: `TERSE_SAFE_MODE=1`                     |
| Optimized output confuses AI  | Test each optimizer manually; validate with real sessions     | Disable problematic optimizer via config           |
| LLM latency too high          | Model warm-up, caching, timeout with fallback to raw          | `fast-only` mode bypasses LLM entirely             |
| LLM hallucinates              | Strong prompts, output validation, length sanity check        | Validation failure → return raw output             |
| Ollama not installed/running  | Health check detects; fast path + passthrough still work      | `fast-only` mode, clear `terse health` diagnostics |
| Performance overhead too high | Profile early, optimize hot paths, <20ms target               | Circuit breaker disables slow paths                |
| Cross-platform differences    | Abstraction layer, `#[cfg]` conditionals, CI on all platforms | Platform-specific tests in CI                      |

### Project Risks

| Risk                                 | Mitigation                                                       | Strategy                                                       |
| ------------------------------------ | ---------------------------------------------------------------- | -------------------------------------------------------------- |
| Rust learning curve                  | Phases introduce concepts incrementally; work with AI assistance | Learn by doing, one concept per phase                          |
| Low adoption (complex install)       | Plugin system, install scripts, Ollama setup automation          | Excellent docs, `terse health` diagnostics                     |
| Maintenance burden (many optimizers) | Modular design, good tests, `terse discover` guides priorities   | LLM smart path as catch-all reduces need for manual optimizers |

---

## Timeline Summary

| Phase | Week  | Deliverable                                 | Key Rust Concepts                         |
| ----- | ----- | ------------------------------------------- | ----------------------------------------- |
| 1     | 1     | Hook passthrough + project setup            | Modules, serde, stdin/stdout, Result      |
| 2     | 2–3   | Git optimizer with measurable savings       | Traits, enums, regex, match, tests        |
| 3     | 3–4   | LLM smart path via Ollama HTTP              | HTTP client, timeouts, Option chaining    |
| 4     | 4–5   | Router + decision engine + circuit breaker  | Enums as types, caching, builder pattern  |
| 5     | 5–6   | Analytics CLI + logging                     | HashMap, iterators, formatted output      |
| 5-1   | 6     | Smart path preprocessing pipeline           | Iterators, HashSet, regex, string slicing |
| 6     | 6–7   | Config system (TOML, hierarchy, profiles)   | TOML parsing, path handling, defaults     |
| 7     | 7–8   | File, build, docker optimizers + whitespace | Code org at scale, DRY, trait objects     |
| 8     | 8–9   | Prompt engineering + LLM quality            | String templating, structured output      |
| 9     | 9–10  | Safety, error handling, reliability         | Custom errors, thiserror, anyhow          |
| 10    | 10–11 | Cross-platform builds + CI/CD               | cfg attributes, GitHub Actions            |
| 11    | 11–12 | Claude Code plugin packaging                | Distribution, manifests                   |
| 12    | 12+   | Copilot CLI + advanced features             | Async, context detection                  |

---

## Verification Checklist

At each phase, validate with:

- **Unit tests:** `cargo test` — optimizer correctness, router decisions, config parsing
- **Integration tests:** End-to-end hook flow with sample JSON input
- **Manual testing:** Run real Claude Code sessions, observe behavior
- **Token measurement:** Compare original vs optimized output sizes in logs
- **`terse stats`:** (Phase 5+) Use analytics to verify savings percentages
- **`terse health`:** Confirm Ollama connectivity, hook registration, config validity
- **Cross-platform CI:** (Phase 10+) All tests pass on Windows/macOS/Linux

**Overall success target:** 60–80% average token savings, <20ms fast path latency, <2s smart path latency, <0.1% failure rate, single binary install.
