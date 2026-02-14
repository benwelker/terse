# Adding a New Optimizer

This guide walks through creating a new command optimizer for TERSE. Optimizers intercept specific shell commands and compact their output to reduce token consumption in AI coding sessions.

## Architecture Overview

TERSE uses a **registry-based optimizer pattern**:

1. The **router** executes the original command and captures raw output.
2. The **`OptimizerRegistry`** iterates through registered optimizers in priority order.
3. The **first** optimizer whose `can_handle()` returns `true` gets to process the output via `optimize_output()`.
4. A **generic fallback** optimizer handles anything that doesn't match a specialized one.

Optimizers are tried in this order: git → file → build → docker → **your optimizer** → generic.

## Step-by-Step Guide

### 1. Create the optimizer file

Create `src/optimizers/your_command.rs`. Every optimizer follows the same structure:

```rust
use anyhow::Result;

use crate::config::schema::YourOptimizerConfig;
use crate::optimizers::{CommandContext, OptimizedOutput, Optimizer};
use crate::utils::token_counter::estimate_tokens;

// ---------------------------------------------------------------------------
// Subcommand classification
// ---------------------------------------------------------------------------

/// Recognized subcommands for your tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YourCommand {
    List,
    Show,
    Status,
}

/// Classify the core command into a variant.
fn classify(lower: &str) -> Option<YourCommand> {
    if lower.starts_with("mytool list") {
        return Some(YourCommand::List);
    }
    if lower.starts_with("mytool show") {
        return Some(YourCommand::Show);
    }
    if lower.starts_with("mytool status") {
        return Some(YourCommand::Status);
    }
    None
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

pub struct YourOptimizer {
    max_list_rows: usize,
    max_detail_lines: usize,
}

impl Default for YourOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl YourOptimizer {
    pub fn new() -> Self {
        Self::from_config(&YourOptimizerConfig::default())
    }

    /// Create from configuration.
    pub fn from_config(cfg: &YourOptimizerConfig) -> Self {
        Self {
            max_list_rows: cfg.max_list_rows,
            max_detail_lines: cfg.max_detail_lines,
        }
    }
}

impl Optimizer for YourOptimizer {
    fn name(&self) -> &'static str {
        "your-tool"
    }

    fn can_handle(&self, ctx: &CommandContext) -> bool {
        let lower = ctx.core.to_ascii_lowercase();
        classify(&lower).is_some()
    }

    fn optimize_output(
        &self,
        ctx: &CommandContext,
        raw_output: &str,
    ) -> Result<OptimizedOutput> {
        let lower = ctx.core.to_ascii_lowercase();
        let cmd = classify(&lower)
            .ok_or_else(|| anyhow::anyhow!("unrecognized subcommand"))?;

        let optimized = match cmd {
            YourCommand::List => compact_list(raw_output, self.max_list_rows),
            YourCommand::Show => compact_show(raw_output, self.max_detail_lines),
            YourCommand::Status => summarize_status(raw_output),
        };

        Ok(OptimizedOutput {
            optimized_tokens: estimate_tokens(&optimized),
            output: optimized,
            optimizer_used: self.name().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Compaction helpers
// ---------------------------------------------------------------------------

fn compact_list(raw_output: &str, max_rows: usize) -> String {
    let lines: Vec<&str> = raw_output.lines().collect();
    if lines.len() <= max_rows {
        return raw_output.to_string();
    }
    let kept: Vec<&str> = lines[..max_rows].to_vec();
    let omitted = lines.len() - max_rows;
    format!("{}\n...+{omitted} more ({} total)", kept.join("\n"), lines.len())
}

fn compact_show(raw_output: &str, max_lines: usize) -> String {
    // Your compaction logic here
    let _ = max_lines;
    raw_output.to_string()
}

fn summarize_status(raw_output: &str) -> String {
    // Your summary logic here
    raw_output.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::CommandContext;

    #[test]
    fn handles_known_commands() {
        let opt = YourOptimizer::new();
        assert!(opt.can_handle(&CommandContext::new("mytool list")));
        assert!(opt.can_handle(&CommandContext::new("mytool show")));
        assert!(!opt.can_handle(&CommandContext::new("other-tool list")));
    }

    #[test]
    fn list_truncates() {
        let lines: Vec<String> = (0..50).map(|i| format!("item-{i}")).collect();
        let input = lines.join("\n");
        let result = compact_list(&input, 20);
        assert!(result.contains("...+30 more (50 total)"));
    }
}
```

### 2. Add the config struct

In `src/config/schema.rs`, add a config struct for your optimizer's tunable limits:

```rust
/// Your-tool optimizer limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct YourOptimizerConfig {
    /// Maximum rows in list output.
    pub max_list_rows: usize,
    /// Maximum lines in detail/show output.
    pub max_detail_lines: usize,
}

impl Default for YourOptimizerConfig {
    fn default() -> Self {
        Self {
            max_list_rows: 30,
            max_detail_lines: 60,
        }
    }
}
```

Then add the field to `OptimizersConfig`:

```rust
pub struct OptimizersConfig {
    pub git: GitOptimizerConfig,
    pub file: FileOptimizerConfig,
    pub build: BuildOptimizerConfig,
    pub docker: DockerOptimizerConfig,
    pub your_tool: YourOptimizerConfig,   // ← add this
    pub generic: GenericOptimizerConfig,
}
```

And add the default TOML section in `default_toml()`:

```toml
[optimizers.your_tool]
max_list_rows = 30
max_detail_lines = 60
```

### 3. Register the optimizer

**`src/optimizers/mod.rs`** — Add module declaration, re-export, and registry entry:

```rust
pub mod your_command;                      // module declaration
pub use your_command::YourOptimizer;       // re-export

// In OptimizerRegistry::from_config():
Self {
    optimizers: vec![
        Box::new(GitOptimizer::from_config(&cfg.git)),
        Box::new(FileOptimizer::from_config(&cfg.file)),
        Box::new(BuildOptimizer::from_config(&cfg.build)),
        Box::new(DockerOptimizer::from_config(&cfg.docker)),
        Box::new(YourOptimizer::from_config(&cfg.your_tool)),  // ← before generic
        Box::new(GenericOptimizer::from_config(&cfg.generic)),  // always last
    ],
}
```

> **Important:** The generic optimizer must always be last in the list. It acts as a fallback and matches everything.

### 4. Add integration tests

Create tests in `tests/optimizer_tests.rs` to verify end-to-end behavior:

```rust
#[test]
fn your_optimizer_handles_commands() {
    let optimizer = YourOptimizer::new();
    assert!(optimizer.can_handle(&CommandContext::new("mytool list")));
    assert!(optimizer.can_handle(&CommandContext::new("cd /app && mytool status")));
    assert!(!optimizer.can_handle(&CommandContext::new("cargo build")));
}
```

## Key Concepts

### CommandContext

Every optimizer receives a `CommandContext` with two fields:

| Field      | Description                                                   | Example                          |
| ---------- | ------------------------------------------------------------- | -------------------------------- |
| `original` | Full command as sent by Claude Code                           | `cd /repo && LANG=C mytool list` |
| `core`     | Extracted command (stripped of `cd`, env vars, pipes, shells) | `mytool list`                    |

Use `ctx.core` for matching/routing. The matching engine (in `src/matching/mod.rs`) handles stripping `cd`, `&&` chains, `bash -c` wrappers, environment variables, and pipe segments automatically.

### The Optimizer Trait

```rust
pub trait Optimizer {
    /// Short identifier for logging/analytics (e.g., "git", "docker").
    fn name(&self) -> &'static str;

    /// Return true if this optimizer can handle the given command.
    /// Use ctx.core.to_ascii_lowercase() for prefix matching.
    fn can_handle(&self, ctx: &CommandContext) -> bool;

    /// Transform raw command output into a compact form.
    /// Called only when can_handle() returned true.
    fn optimize_output(&self, ctx: &CommandContext, raw_output: &str)
        -> Result<OptimizedOutput>;
}
```

### OptimizedOutput

```rust
pub struct OptimizedOutput {
    pub output: String,        // The compacted text
    pub optimized_tokens: usize, // Token estimate (use estimate_tokens())
    pub optimizer_used: String,  // self.name().to_string()
}
```

### Configurable Limits

All numeric limits (max rows, max lines, truncation thresholds) should be **configurable** rather than hardcoded. The pattern is:

1. Define a config struct with `#[derive(Debug, Clone, Serialize, Deserialize)]` and `#[serde(default)]`.
2. Implement `Default` with sensible values.
3. Store limits as struct fields on the optimizer.
4. Accept limits via `from_config(&YourConfig)` constructor.
5. Pass limits as parameters to helper functions — never read config at call time.

Users can then tune limits in `~/.terse/config.toml` or per-project `.terse.toml`:

```toml
[optimizers.your_tool]
max_list_rows = 50
max_detail_lines = 100
```

## Design Principles

### Golden Rule: Never Break the Session

Any error in an optimizer must result in a **graceful passthrough** — return the raw output unchanged. Never panic, never swallow output. The `Result` return from `optimize_output()` ensures the registry falls through on errors.

### Compaction Strategies

Existing optimizers use two main strategies. Pick the one that fits:

| Strategy                   | When to use                                       | Example                               |
| -------------------------- | ------------------------------------------------- | ------------------------------------- |
| **Output post-processing** | Transform output after the command runs           | `git diff` → compact hunks            |
| **Command substitution**   | Replace the command with a more efficient variant | `git log` → `git log --oneline -n 20` |

Most optimizers use **output post-processing**. Command substitution is used by `git log` and `git status` where a different command produces inherently smaller output.

### What Makes Good Compaction

- **Preserve errors and failures** — these are the most important information.
- **Keep summary lines** — totals, counts, status lines.
- **Truncate with context** — show head + tail, note how many lines were omitted.
- **Strip noise** — progress bars, repeated separators, timestamps on non-error lines.
- **Never remove information the AI needs to act on** — if in doubt, keep it.

## Checklist

Before submitting a new optimizer:

- [ ] Created `src/optimizers/your_command.rs` implementing the `Optimizer` trait
- [ ] Added config struct in `src/config/schema.rs` with `#[serde(default)]`
- [ ] Added field to `OptimizersConfig`
- [ ] Added TOML defaults to `default_toml()`
- [ ] Added module + re-export in `src/optimizers/mod.rs`
- [ ] Registered in `OptimizerRegistry::from_config()` (before generic)
- [ ] Unit tests in the optimizer file (`#[cfg(test)] mod tests`)
- [ ] Integration tests in `tests/optimizer_tests.rs`
- [ ] All limits are configurable (no hardcoded magic numbers)
- [ ] `cargo test` passes
- [ ] `cargo clippy` is clean
- [ ] `cargo fmt --check` passes

## Reference Implementations

| Optimizer | File                        | Complexity | Good example for                                      |
| --------- | --------------------------- | ---------- | ----------------------------------------------------- |
| Generic   | `src/optimizers/generic.rs` | Simple     | Minimal trait implementation, config pattern          |
| File      | `src/optimizers/file.rs`    | Medium     | Multiple subcommands, varied compaction strategies    |
| Docker    | `src/optimizers/docker.rs`  | Medium     | Subcommand enum, table reformatting, noise stripping  |
| Build     | `src/optimizers/build.rs`   | Medium     | Error/warning extraction, cross-tool detection        |
| Git       | `src/optimizers/git.rs`     | Complex    | Command substitution + post-processing, full featured |
