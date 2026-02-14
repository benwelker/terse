# TERSE

[![Rust Edition](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.0-blue)](Cargo.toml)
[![CLI](https://img.shields.io/badge/type-CLI-informational)](#cli-usage)
[![Smart%20Path](https://img.shields.io/badge/smart_path-ollama%20opt--in-lightgrey)](#smart-path-setup-ollama)

**Token Efficiency through Refined Stream Engineering**

TERSE is a Rust CLI that intercepts shell commands from Claude Code hooks, runs them through an optimization pipeline, and returns compact output designed to reduce token usage while preserving key signal.

**🌈 99.9% vibe coded with Claude Opus 4.6. Don't come for me.**

## Quick demo

```bash
# 1) Build
cargo build --release

# 2) Initialize config (optional but recommended)
terse config init
#    (also prints quick PATH setup commands so `terse` works from anywhere)

# 3) Preview routing + optimization for a command
terse test git status

# 4) View aggregate token savings after usage
terse stats
```

Example `terse test git status` (shape):

```text
TERSE Optimization Preview
==================================================
	Command:       git status
	Hook decision: rewrite
	Path taken:    fast
	Optimizer:     git
	Tokens:        120 -> 28 (76.67% savings)

--- Output ---
branch: main...origin/main
modified (2): src/main.rs, README.md
untracked (1): notes.txt
```

## What it does

- Rewrites safe `Bash` tool commands to `terse run "<original command>"` via Claude PreToolUse hook protocol
- Routes output through:
  - **Fast path** (rule-based optimizer, currently Git-focused)
  - **Smart path** (local LLM via Ollama, opt-in)
  - **Passthrough** (for unsafe/small/unoptimizable cases)
- Applies a preprocessing pipeline (noise removal, path filtering, dedup, truncation, whitespace normalization)
- Logs analytics and events for stats/trends/discovery

## Current implemented scope

### Fast path optimizers

Currently implemented optimizer module:

- `git` (single optimizer with subcommand-specific handling)

Supported Git command families:

- `git status`
- `git log`
- `git diff`
- `git branch`
- `git show`
- `git stash`
- `git worktree`
- short operation summaries for `git push|pull|fetch|add|commit`

### Smart path

- Uses Ollama HTTP API (`/api/chat`)
- Disabled by default; enabled via config or `TERSE_SMART_PATH=1`
- Performs validation before accepting LLM output

### Safety gates

Never optimized:

- Destructive/editor commands like `rm`, `mv`, `vim`, `code`, etc.
- Commands with file output redirection (`>`, `>>`)
- Heredoc-heavy commands
- Existing `terse run ...` calls (infinite-loop guard)

## Architecture overview

1. Claude Code triggers PreToolUse hook (`terse hook`)
2. Hook either:
   - returns `{}` (passthrough), or
   - rewrites command to `terse run "..."`
3. `terse run` executes original command
4. Router preprocesses output and selects path based on config + output size
5. Optimized output is printed to stdout and logged

## Installation (from source)

```bash
git clone https://github.com/bwelker/terse.git
cd terse
cargo build --release
```

Binary path after release build:

- Windows: `target\release\terse.exe`
- macOS/Linux: `target/release/terse`

## Hook setup (Claude Code)

Add a PreToolUse hook entry to your Claude settings file (`~/.claude/settings.json`), pointing to your TERSE binary.

Example (Windows):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "C:\\path\\to\\terse.exe hook"
          }
        ]
      }
    ]
  }
}
```

## CLI usage

### Core

```bash
terse hook
terse run <command>
```

### Analytics

```bash
terse stats [--format table|json|csv] [--days N]
terse analyze --days N [--format table|json|csv]
terse discover [--format table|json|csv] [--days N]
```

### Diagnostics

```bash
terse health
terse test <command>
```

### Config management

```bash
terse config show
terse config init [--force]
terse config set <dotted.key> <value>
terse config reset
```

## Smart path setup (Ollama)

1. Install Ollama
2. Pull a model (default expected model is `llama3.2:1b`)

```bash
ollama pull llama3.2:1b
```

3. Enable smart path:

```bash
TERSE_SMART_PATH=1
```

or in config:

```toml
[smart_path]
enabled = true
model = "llama3.2:1b"
ollama_url = "http://localhost:11434"
```

## Configuration

Config hierarchy used by TERSE:

1. Built-in defaults
2. Global config: `~/.terse/config.toml`
3. Project config: `.terse.toml`
4. Environment overrides: `TERSE_*`

Generate a starter config:

```bash
terse config init
```

Useful env vars:

- `TERSE_ENABLED`
- `TERSE_MODE` (`hybrid`, `fast-only`, `smart-only`, `passthrough`)
- `TERSE_PROFILE` (`fast`, `balanced`, `quality`)
- `TERSE_SAFE_MODE`
- `TERSE_SMART_PATH`
- `TERSE_SMART_PATH_MODEL`
- `TERSE_SMART_PATH_URL`
- `TERSE_SMART_PATH_TIMEOUT_MS`

## Runtime files

- Config: `~/.terse/config.toml`
- Legacy smart-path JSON fallback: `~/.terse/config.json`
- Command analytics log: `~/.terse/command-log.jsonl`
- Raw hook event log: `~/.terse/events.jsonl`
- Hook diagnostic log: `~/.terse/hook.log`

## Development

```bash
cargo build
cargo test
cargo clippy
cargo fmt --check
```

Live LLM integration tests (requires Ollama running):

```bash
TERSE_TEST_LLM=1 cargo test llm_live
```

## Status

This repository is an actively evolving implementation. The long-range roadmap and phase plan are tracked in:

- `.claude/plans/TERSE-FINAL-Plan.md`

Current codebase already includes hook integration, router, preprocessing pipeline, Git optimizer, LLM smart path integration, analytics commands, and TOML-based configuration management.
