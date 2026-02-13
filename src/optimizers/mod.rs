use anyhow::Result;

pub mod git;

pub use git::GitOptimizer;

#[derive(Debug, Clone)]
pub struct OptimizedOutput {
    pub output: String,
    pub original_tokens: usize,
    pub optimized_tokens: usize,
    pub optimizer_used: String,
}

/// Trait for command optimizers.
///
/// Each optimizer handles a set of commands. When invoked via
/// [`execute_and_optimize`](Optimizer::execute_and_optimize), the optimizer
/// runs the command itself (potentially as a substituted variant) and returns
/// the optimized output along with token analytics.
pub trait Optimizer {
    fn name(&self) -> &'static str;
    fn can_handle(&self, command: &str) -> bool;

    /// Execute the command (or an optimized substitute) and return the
    /// optimized output with token analytics.
    ///
    /// The optimizer decides the strategy:
    /// - **Command substitution**: run a more compact command instead
    ///   (e.g., `git status --short --branch` instead of `git status`)
    /// - **Output post-processing**: run the original command, then
    ///   transform the output (e.g., truncate a large diff)
    fn execute_and_optimize(&self, command: &str) -> Result<OptimizedOutput>;
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

    pub fn can_handle(&self, command: &str) -> bool {
        self.optimizers
            .iter()
            .any(|optimizer| optimizer.can_handle(command))
    }

    /// Find the first optimizer that can handle the command, execute it, and
    /// return the optimized output. Returns `None` if no optimizer matches or
    /// all matching optimizers fail.
    pub fn execute_first(&self, command: &str) -> Option<OptimizedOutput> {
        for optimizer in &self.optimizers {
            if !optimizer.can_handle(command) {
                continue;
            }

            if let Ok(result) = optimizer.execute_and_optimize(command) {
                return Some(result);
            }
        }

        None
    }
}
