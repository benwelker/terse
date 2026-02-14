use anyhow::Result;
use clap::{Parser, Subcommand};

mod analytics;
mod cli;
mod config;
mod hook;
mod llm;
mod matching;
mod optimizers;
mod preprocessing;
mod router;
mod run;
mod safety;
mod utils;
mod web;

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
    /// Manage terse configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Launch the web dashboard for analytics and configuration
    Web {
        /// Address to bind the server to
        #[arg(long, default_value = "127.0.0.1:9746")]
        addr: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Show effective configuration (merged from all sources)
    Show,
    /// Initialize a default config file at ~/.terse/config.toml
    Init {
        /// Overwrite existing config file
        #[arg(long)]
        force: bool,
    },
    /// Set a configuration value (e.g. `terse config set general.mode fast-only`)
    Set {
        /// Dotted key path (e.g. general.mode, smart_path.model)
        key: String,
        /// Value to set (auto-detected as bool/int/float/string)
        value: String,
    },
    /// Reset configuration to defaults
    Reset,
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
        Commands::Config { action } => match action {
            ConfigAction::Show => cli::run_config_show(),
            ConfigAction::Init { force } => cli::run_config_init(force),
            ConfigAction::Set { key, value } => cli::run_config_set(&key, &value),
            ConfigAction::Reset => cli::run_config_reset(),
        },
        Commands::Web { addr } => web::serve(&addr),
    }
}
