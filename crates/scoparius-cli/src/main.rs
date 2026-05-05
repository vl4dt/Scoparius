//! Scoparius CLI — disk analyzer and cleanup utility.

mod commands;
mod output;
mod tui;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "scoparius", version, about = "Cross-platform disk analyzer and intelligent cleanup utility")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: commands::Commands,

    /// Increase output verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Preview operations without executing
    #[arg(long, global = true)]
    dry_run: bool,

    /// Custom config file path
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = match cli.verbose {
        0 => "scoparius=info",
        1 => "scoparius=debug",
        _ => "scoparius=trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .with_target(false)
        .init();

    // Load config
    let config = match &cli.config {
        Some(path) => scoparius_core::ScopariusConfig::load_from(path)?,
        None => scoparius_core::ScopariusConfig::load()?,
    };

    // Dispatch command
    commands::run(cli.command, config, cli.dry_run, cli.json, cli.quiet).await
}
