use anyhow::Result;
use clap::{Parser, Subcommand};

mod analytics;
mod cli;
mod hook;
mod matching;
mod optimizers;
mod run;
mod utils;

#[derive(Debug, Parser)]
#[command(name = "terse")]
#[command(about = "Token Efficiency through Refined Stream Engineering")]
struct App {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// PreToolUse hook handler — reads JSON from stdin, returns rewrite or passthrough
    Hook,
    /// Execute a command with optimization and print the result to stdout
    Run {
        /// The command to execute and optimize
        #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show token savings statistics
    Stats,
}

fn main() -> Result<()> {
    let app = App::parse();

    match app.command {
        Commands::Hook => hook::run(),
        Commands::Run { args } => {
            let command = args.join(" ");
            run::execute(&command)
        }
        Commands::Stats => cli::run_stats(),
    }
}
