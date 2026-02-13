use anyhow::Result;
use clap::{Parser, Subcommand};

mod cli;
mod hook;

#[derive(Debug, Parser)]
#[command(name = "terse")]
#[command(about = "Token Efficiency through Refined Stream Engineering")]
struct App {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Hook,
    Stats,
}

fn main() -> Result<()> {
    let app = App::parse();

    match app.command {
        Commands::Hook => hook::run(),
        Commands::Stats => cli::run_stats(),
    }
}
