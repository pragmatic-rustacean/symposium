//! Symposium ACP - Main entry point

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp")]
#[command(about = "Symposium ACP meta proxy - orchestrates dynamic component chains")]
struct Cli {
    #[command(flatten)]
    logging: symposium_acp::LoggingArgs,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    symposium_acp::run(&cli.logging).await
}
