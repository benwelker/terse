use anyhow::Result;

use crate::router;

pub fn run_stats() -> Result<()> {
    println!("terse stats is not available yet (planned for Phase 5).");
    Ok(())
}

/// Preview the optimization pipeline for a command.
///
/// Shows the hook-level decision, executes the command through the router,
/// and displays the path taken, token savings, and optimized output.
pub fn run_test(command: &str) -> Result<()> {
    let preview = router::preview(command)?;

    println!("Command:       {command}");
    println!("Hook decision: {}", preview.hook_decision);
    println!("Path taken:    {}", preview.execution.path);
    println!("Optimizer:     {}", preview.execution.optimizer_name);

    let savings = if preview.execution.original_tokens == 0 {
        0.0
    } else {
        let saved = preview
            .execution
            .original_tokens
            .saturating_sub(preview.execution.optimized_tokens);
        (saved as f64 / preview.execution.original_tokens as f64) * 100.0
    };

    println!(
        "Tokens:        {} -> {} ({:.1}% savings)",
        preview.execution.original_tokens,
        preview.execution.optimized_tokens,
        savings
    );

    if let Some(latency) = preview.execution.latency_ms {
        println!("Latency:       {}ms", latency);
    }

    println!();
    println!("--- Output ---");
    print!("{}", preview.execution.output);

    if !preview.execution.stderr.is_empty() {
        println!();
        println!("--- Stderr ---");
        print!("{}", preview.execution.stderr);
    }

    Ok(())
}
