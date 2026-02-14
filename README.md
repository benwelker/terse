# terse

**🌈 99.9% vibe coded with Claude Opus 4.6. Don't come for me.**

[![Rust Edition](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![CI](https://img.shields.io/github/actions/workflow/status/benwelker/terse/ci.yml?branch=master&label=CI&logo=github)](https://github.com/benwelker/terse/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/actions/workflow/status/benwelker/terse/release.yml?label=Release&logo=github)](https://github.com/benwelker/terse/actions/workflows/release.yml)
[![Version](https://img.shields.io/github/v/release/benwelker/terse?label=version)](https://github.com/benwelker/terse/releases)
[![CLI](https://img.shields.io/badge/type-CLI-informational)](#cli-usage)
[![Smart%20Path](https://img.shields.io/badge/smart_path-ollama%20opt--in-lightgrey)](#smart-path-setup-ollama)

terse is a Rust CLI that intercepts shell commands from Claude Code hooks, runs them through an optimization pipeline, and returns compact output designed to reduce token usage while preserving key signal.

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
terse Optimization Preview
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

## Installation

### One-liner install (recommended)

Downloads the latest release binary, places it in `~/.terse/bin/`, creates a default config, and registers the Claude Code hook automatically.

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/benwelker/terse/master/scripts/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/benwelker/terse/master/scripts/install.ps1 | iex
```

Both scripts will:

1. Download the correct binary for your platform/architecture
2. Place it in `~/.terse/bin/`
3. Generate a default `~/.terse/config.toml`
4. Check for Ollama availability
5. Register the PreToolUse hook in `~/.claude/settings.json`

### From source

```bash
git clone https://github.com/benwelker/terse.git
cd terse
cargo build --release
```

Binary path after release build:

- Windows: `target\release\terse.exe`
- macOS/Linux: `target/release/terse`

## Hook setup (Claude Code)

Add a PreToolUse hook entry to your Claude settings file (`~/.claude/settings.json`), pointing to your terse binary.

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

### Web dashboard

```bash
terse web                          # opens http://127.0.0.1:9746
terse web --addr 0.0.0.0:8080      # custom bind address
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

Config hierarchy used by terse:

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

## Web dashboard

terse ships with a built-in web dashboard — a single-page app compiled into the binary with zero external dependencies.

```bash
terse web
```

Opens `http://127.0.0.1:9746` in your default browser and serves:

- **Dashboard** — total commands, token savings, path distribution bar, top commands table
- **Trends** — daily bar chart of token savings over the last 30 days
- **Discovery** — passthrough/smart-path commands that could benefit from new fast-path optimizers
- **Configuration** — form-based editor for all key settings, saving directly to `~/.terse/config.toml`

The server is synchronous (`tiny_http`), single-threaded, and binds to localhost only by default. Override with `--addr`.

## Status

This repository is an actively evolving implementation. The long-range roadmap and phase plan are tracked in:

- `.claude/plans/terse-FINAL-Plan.md`

Current codebase already includes hook integration, router, preprocessing pipeline, Git optimizer, LLM smart path integration, analytics commands, TOML-based configuration management, web dashboard, cross-platform support, and CI/CD workflows.
