//! The austeris binary.
//!
//! One executable runs every service; `serve <service>` picks which (ADR 0005).
//! The services stay separate processes with separate schemas and separate
//! contracts — what they share is a build, an image and a version.

mod gateway;
mod service;

use anyhow::Result;
use austeris_common::{Config, telemetry};
use clap::{Parser, Subcommand};

use crate::service::Service;

/// Self-hosted home finance.
#[derive(Debug, Parser)]
#[command(name = "austeris", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Runs one service in the foreground.
    Serve {
        /// Which service this process is.
        service: Service,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { service } => serve(service).await,
    }
}

async fn serve(service: Service) -> Result<()> {
    let config = Config::from_env()?;
    let app = match service {
        Service::Gateway => gateway::router(),
    };

    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!(service = service.as_str(), address = %listener.local_addr()?, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}
