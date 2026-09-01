//! The austeris binary.
//!
//! One executable runs every service; `serve <service>` picks which (ADR 0005).
//! The services stay separate processes with separate schemas and separate
//! contracts - what they share is a build, an image and a version.

mod gateway;
mod migrate;
mod ratelimit;
mod service;

use anyhow::{Context, Result};
use austeris_common::{Config, db, telemetry};
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
    /// Applies pending migrations, or rolls a schema back.
    Migrate(migrate::Args),
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { service } => serve(service).await,
        Command::Migrate(args) => migrate::run(&args).await,
    }
}

async fn serve(service: Service) -> Result<()> {
    let config = Config::from_env()?;

    // A service that owns a schema also serves gRPC to its peers, on a second
    // port. The two are separate listeners rather than one multiplexed port:
    // the REST surface is what the gateway forwards to, the gRPC one is what
    // other services call, and only the first should ever be reachable from
    // outside the compose network.
    let (app, grpc) = match service {
        Service::Gateway => (gateway::router(), None),
        Service::Identity => {
            let pool = db::connect(&config, austeris_identity::SCHEMA).await?;
            // Each service migrates its own schema on the way up: one binary,
            // one owner per schema, and no separate step to forget on a deploy.
            // `austeris migrate --dry-run` exists to see it coming first.
            austeris_common::migrate::run(&pool, &austeris_identity::MIGRATOR)
                .await
                .context("migrating the identity schema")?;

            if let Some(password) = austeris_identity::routes::ensure_first_user(&pool, &first_user_email()).await? {
                announce_first_user(&first_user_email(), &password);
            }

            let grpc = tonic::transport::Server::builder()
                .add_service(austeris_identity::grpc::Service::new(pool.clone()))
                .serve(crate::service::grpc_bind().parse().context("AUSTERIS_GRPC_BIND is not an address")?);

            (austeris_identity::routes::router(pool, secure_cookies()), Some(grpc))
        }
    };

    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!(service = service.as_str(), address = %listener.local_addr()?, "listening");

    // `into_make_service_with_connect_info` rather than the plain one: the
    // gateway's rate limiter needs the peer address, and without this the
    // extractor has nothing to read.
    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();

    match grpc {
        // Either listener stopping means the service is no longer whole, so
        // whichever finishes first ends the process rather than leaving half
        // of it answering.
        Some(grpc) => tokio::try_join!(async { axum::serve(listener, app).await.context("the HTTP listener stopped") }, async {
            grpc.await.context("the gRPC listener stopped")
        })
        .map(|_| ()),
        None => axum::serve(listener, app).await.context("the HTTP listener stopped"),
    }
}

/// The address the first account is created under.
fn first_user_email() -> String {
    std::env::var("AUSTERIS_FIRST_USER").unwrap_or_else(|_| "owner@austeris.local".to_owned())
}

/// Whether session cookies are marked `Secure`.
///
/// Off by default: a self-hosted install on a home network is usually plain
/// HTTP, and an unconditional flag would make every sign-in fail with nothing
/// in the response saying why - the browser simply drops the cookie.
fn secure_cookies() -> bool {
    std::env::var("AUSTERIS_SECURE_COOKIES").is_ok_and(|value| value == "true")
}

/// Prints the generated password once, where an operator will see it.
///
/// Deliberately not through `tracing`: this must not land in a log file that is
/// shipped somewhere, and it must be legible among structured lines.
fn announce_first_user(email: &str, password: &str) {
    println!();
    println!("  An account was created, because this installation had none:");
    println!();
    println!("      email:    {email}");
    println!("      password: {password}");
    println!();
    println!("  This is the only time it is shown. Sign in and change it.");
    println!();
}
