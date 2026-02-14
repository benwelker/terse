use anyhow::Result;

use crate::config::schema::OptimizersConfig;
use crate::matching;

pub mod build;
pub mod docker;
pub mod file;
pub mod generic;
pub mod git;

pub use build::BuildOptimizer;
pub use docker::DockerOptimizer;
pub use file::FileOptimizer;
pub use generic::GenericOptimizer;
pub use git::GitOptimizer;

/// Output produced by an optimizer after post-processing raw command output.
#[derive(Debug, Clone)]
pub struct OptimizedOutput {
    pub output: String,
    pub optimized_tokens: usize,
    pub optimizer_used: String,
}

/// Pre-extracted command context passed to optimizers.
///
/// Created once by the [`OptimizerRegistry`] to eliminate redundant calls to
/// [`extract_core_command`](crate::matching::extract_core_command). Optimizers
/// use `core` for matching/routing and `original` for context.
#[derive(Debug, Clone, Copy)]
pub struct CommandContext<'a> {
    /// The full original command as sent by Claude Code.
    /// Preserved for context (e.g., `cd /repo && git status`).
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
/// Each optimizer handles a set of commands. The router runs the command
/// first, then passes the raw output to the optimizer for post-processing.
/// Optimizers transform the output into a more compact representation.
pub trait Optimizer {
    fn name(&self) -> &'static str;

    /// Check whether this optimizer can handle the command.
    ///
    /// Use `ctx.core` for prefix matching (already lowered/extracted).
    fn can_handle(&self, ctx: &CommandContext) -> bool;

    /// Post-process raw command output into a compact, token-efficient form.
    ///
    /// The router has already executed the command and passes the raw output.
    /// The optimizer transforms it (e.g., compacting a diff, summarizing
    /// branch listings) and returns the optimized output with token analytics.
    fn optimize_output(&self, ctx: &CommandContext, raw_output: &str) -> Result<OptimizedOutput>;
}

/// Registry of all available optimizers.
pub struct OptimizerRegistry {
    optimizers: Vec<Box<dyn Optimizer>>,
}

impl Default for OptimizerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizerRegistry {
    /// Create a registry using default limits for all optimizers.
    pub fn new() -> Self {
        Self::from_config(&OptimizersConfig::default())
    }

    /// Create a registry using limits from the given config.
    ///
    /// Only optimizers whose `enabled` flag is `true` are loaded.
    pub fn from_config(cfg: &OptimizersConfig) -> Self {
        let mut optimizers: Vec<Box<dyn Optimizer>> = Vec::new();

        // Specialized optimizers (tried first, in priority order)
        if cfg.git.enabled {
            optimizers.push(Box::new(GitOptimizer::from_config(&cfg.git)));
        }
        if cfg.file.enabled {
            optimizers.push(Box::new(FileOptimizer::from_config(&cfg.file)));
        }
        if cfg.build.enabled {
            optimizers.push(Box::new(BuildOptimizer::from_config(&cfg.build)));
        }
        if cfg.docker.enabled {
            optimizers.push(Box::new(DockerOptimizer::from_config(&cfg.docker)));
        }
        // Generic fallback (tried last — catches everything)
        if cfg.generic.enabled {
            optimizers.push(Box::new(GenericOptimizer::from_config(&cfg.generic)));
        }

        Self { optimizers }
    }

    /// Check whether any registered optimizer can handle the command.
    ///
    /// Extracts the core command once via [`CommandContext`] and passes it
    /// to each optimizer.
    pub fn can_handle(&self, command: &str) -> bool {
        let ctx = CommandContext::new(command);
        self.optimizers.iter().any(|o| o.can_handle(&ctx))
    }

    /// Find the first optimizer that can handle the command, post-process the
    /// raw output, and return the optimized result. Returns `None` if no
    /// optimizer matches or all matching optimizers fail.
    pub fn optimize_first(&self, command: &str, raw_output: &str) -> Option<OptimizedOutput> {
        let ctx = CommandContext::new(command);

        for optimizer in &self.optimizers {
            if !optimizer.can_handle(&ctx) {
                continue;
            }

            if let Ok(result) = optimizer.optimize_output(&ctx, raw_output) {
                return Some(result);
            }
        }

        None
    }
}
