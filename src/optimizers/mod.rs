use anyhow::Result;

use crate::matching;

pub mod git;

pub use git::GitOptimizer;

#[derive(Debug, Clone)]
pub struct OptimizedOutput {
    pub output: String,
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub optimizer_used: String,
}

/// Pre-extracted command context passed to optimizers.
///
/// Created once by the [`OptimizerRegistry`] to eliminate redundant calls to
/// [`extract_core_command`](crate::matching::extract_core_command). Optimizers
/// use `core` for matching/routing and `original` for execution.
#[derive(Debug, Clone, Copy)]
pub struct CommandContext<'a> {
    /// The full original command as sent by Claude Code.
    /// Used for execution — preserves `cd`, env vars, pipes, etc.
    pub original: &'a str,

    /// The core command extracted by the matching engine.
    /// Used for matching/routing (e.g., `"git status"`).
    pub core: &'a str,
}

impl<'a> CommandContext<'a> {
    /// Build a context by extracting the core command once.
    pub fn new(command: &'a str) -> Self {
        Self {
            original: command,
            core: matching::extract_core_command(command),
        }
    }
}

/// Trait for command optimizers.
///
/// Each optimizer handles a set of commands. The registry passes a
/// [`CommandContext`] with the pre-extracted core command so optimizers never
/// need to call `extract_core_command` themselves.
///
/// When invoked via [`execute_and_optimize`](Optimizer::execute_and_optimize),
/// the optimizer runs the command (potentially as a substituted variant) and
/// returns the optimized output along with token analytics.
pub trait Optimizer {
    fn name(&self) -> &'static str;

    /// Check whether this optimizer can handle the command.
    ///
    /// Use `ctx.core` for prefix matching (already lowered/extracted).
    fn can_handle(&self, ctx: &CommandContext) -> bool;

    /// Execute the command (or an optimized substitute) and return the
    /// optimized output with token analytics.
    ///
    /// The optimizer decides the strategy:
    /// - **Command substitution**: run a more compact command instead
    ///   (e.g., `git status --short --branch` instead of `git status`)
    /// - **Output post-processing**: run the original command, then
    ///   transform the output (e.g., truncate a large diff)
    ///
    /// Use `ctx.original` when executing — it preserves `cd`, env vars, etc.
    fn execute_and_optimize(&self, ctx: &CommandContext) -> Result<OptimizedOutput>;
}

pub struct OptimizerRegistry {
    optimizers: Vec<Box<dyn Optimizer>>,
}

impl OptimizerRegistry {
    pub fn new() -> Self {
        Self {
            optimizers: vec![Box::new(GitOptimizer::new())],
        }
    }

    /// Check whether any registered optimizer can handle the command.
    ///
    /// Extracts the core command once via [`CommandContext`] and passes it
    /// to each optimizer.
    pub fn can_handle(&self, command: &str) -> bool {
        let ctx = CommandContext::new(command);
        self.optimizers.iter().any(|o| o.can_handle(&ctx))
    }

    /// Find the first optimizer that can handle the command, execute it, and
    /// return the optimized output. Returns `None` if no optimizer matches or
    /// all matching optimizers fail.
    ///
    /// Extracts the core command once via [`CommandContext`] — individual
    /// optimizers never call `extract_core_command` themselves.
    pub fn execute_first(&self, command: &str) -> Option<OptimizedOutput> {
        let ctx = CommandContext::new(command);

        for optimizer in &self.optimizers {
            if !optimizer.can_handle(&ctx) {
                continue;
            }

            if let Ok(result) = optimizer.execute_and_optimize(&ctx) {
                return Some(result);
            }
        }

        None
    }
}
