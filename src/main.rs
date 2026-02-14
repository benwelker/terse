use anyhow::Result;
use clap::{Parser, Subcommand};

mod analytics;
mod cli;
mod hook;
mod llm;
mod matching;
mod optimizers;
mod preprocessing;
mod router;
mod run;
mod safety;
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
    Stats {
        /// Output format: table (default), json, csv
        #[arg(long, default_value = "table")]
        format: String,
        /// Only include the last N days of data
        #[arg(long)]
        days: Option<u32>,
    },
    /// Analyze time-based trends in token savings
    Analyze {
        /// Number of days to analyze (default: 7)
        #[arg(long, default_value = "7")]
        days: u32,
        /// Output format: table (default), json, csv
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Discover high-frequency commands not yet on the fast path
    Discover {
        /// Output format: table (default), json, csv
        #[arg(long, default_value = "table")]
        format: String,
        /// Only include the last N days of data
        #[arg(long)]
        days: Option<u32>,
    },
    /// Check system health: Ollama, config, circuit breaker
    Health,
    /// Preview optimization for a command — show path selection and optimized output
    Test {
        /// The command to preview
        #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() -> Result<()> {
    let app = App::parse();

    match app.command {
        Commands::Hook => hook::run(),
        Commands::Run { args } => {
            let command = args.join(" ");
            run::execute(&command)
        }
        Commands::Stats { format, days } => {
            let fmt = cli::OutputFormat::from_str_opt(Some(&format));
            cli::run_stats(fmt, days)
        }
        Commands::Analyze { days, format } => {
            let fmt = cli::OutputFormat::from_str_opt(Some(&format));
            cli::run_analyze(days, fmt)
        }
        Commands::Discover { format, days } => {
            let fmt = cli::OutputFormat::from_str_opt(Some(&format));
            cli::run_discover(fmt, days)
        }
        Commands::Health => cli::run_health(),
        Commands::Test { args } => {
            let command = args.join(" ");
            cli::run_test(&command)
        }
    }
}
