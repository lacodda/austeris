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
    // Each service's gRPC server is a different concrete type, so the branches
    // hand back a spawned task rather than the future itself.
    let (app, grpc) = match service {
        Service::Gateway => (gateway::router(), None),
        Service::Identity => {
            let pool = schema_ready(&config, service).await?;

            if let Some(password) = austeris_identity::routes::ensure_first_user(&pool, &first_user_email()).await? {
                announce_first_user(&first_user_email(), &password);
            }

            let address = grpc_address()?;
            let server = tonic::transport::Server::builder()
                .add_service(austeris_identity::grpc::Service::new(pool.clone()))
                .serve(address);

            (
                austeris_identity::routes::router(pool, secure_cookies()),
                Some(tokio::spawn(async move { server.await.context("the gRPC listener stopped") })),
            )
        }
        Service::Market => {
            let pool = schema_ready(&config, service).await?;

            let sources = austeris_market::routes::Sources::from_env();
            // Said once, at startup, rather than on every refresh: a source
            // without its key is switched off, not broken.
            match sources.available().as_slice() {
                [] => tracing::warn!("no price source is configured; prices will not be refreshed"),
                names => tracing::info!(sources = names.join(", "), "price sources available"),
            }

            let address = grpc_address()?;
            let server = tonic::transport::Server::builder()
                .add_service(austeris_market::grpc::Service::new(pool.clone()))
                .serve(address);

            (
                austeris_market::routes::router(pool, sources),
                Some(tokio::spawn(async move { server.await.context("the gRPC listener stopped") })),
            )
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
            grpc.await.context("the gRPC task stopped")?
        })
        .map(|_| ()),
        None => axum::serve(listener, app).await.context("the HTTP listener stopped"),
    }
}

/// Opens a service's pool and brings its schema up to this build's version.
///
/// Every service does this on the way up: one binary, one owner per schema, and
/// no separate step to forget on a deploy. `austeris migrate --dry-run` exists
/// to see it coming first.
async fn schema_ready(config: &Config, service: Service) -> Result<sqlx::PgPool> {
    let (Some(schema), Some(migrator)) = (service.schema(), service.migrator()) else {
        anyhow::bail!("{service} owns no schema");
    };

    let pool = db::connect(config, schema).await?;
    austeris_common::migrate::run(&pool, migrator)
        .await
        .with_context(|| format!("migrating the {schema} schema"))?;
    Ok(pool)
}

/// Where this process serves gRPC.
fn grpc_address() -> Result<std::net::SocketAddr> {
    crate::service::grpc_bind().parse().context("AUSTERIS_GRPC_BIND is not an address")
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
