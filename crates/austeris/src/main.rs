//! The austeris binary.
//!
//! One executable runs every service (ADR 0005). `serve` runs all of them in
//! this one process, `serve <service>` runs one (ADR 0008). Either way the
//! services keep separate schemas, separate pools and separate contracts - what
//! they share is a build, an image and a version.

mod add;
mod demo;
mod gateway;
mod migrate;
mod openapi;
mod ratelimit;
mod service;

use anyhow::{Context, Result};
use austeris_common::{Config, db, telemetry};
use axum::Router;
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tonic::transport::Server;
use tonic::transport::server::TcpIncoming;

use crate::service::{Peers, Service};

/// Self-hosted home finance.
#[derive(Debug, Parser)]
#[command(name = "austeris", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Runs the installation in the foreground: every service, or one.
    Serve {
        /// Run only this service, for an installation with a container per
        /// service. Every service, in this one process, when omitted.
        service: Option<Service>,
    },
    /// Fills an empty installation with a fictional household.
    ///
    /// For a first look and for screenshots: everything it writes is made up,
    /// and it refuses to run on an installation where a real person has an
    /// account. Running it again tops the books up to today.
    Demo,
    /// Applies pending migrations, or rolls a schema back.
    Migrate(migrate::Args),
    /// Records an entry from one typed line: `austeris add 45000 food lunch`.
    Add(add::AddArgs),
    /// Records money changed between two accounts in different currencies:
    /// `austeris exchange 600000 cash 100 dollars`.
    Exchange(add::ExchangeArgs),
    /// Signs in, so `add` has a session to record with.
    Login(add::LoginArgs),
    /// Prints the `OpenAPI` document to stdout.
    ///
    /// The documentation site builds its API reference from this, so the
    /// reference is generated from the binary being released rather than from a
    /// copy checked in beside it that nothing keeps honest.
    Openapi,
}

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { service } => serve(service).await,
        Command::Demo => demo::run().await,
        Command::Migrate(args) => migrate::run(&args).await,
        Command::Add(args) => add::add(&args).await,
        Command::Exchange(args) => add::exchange(&args).await,
        Command::Login(args) => add::login(&args).await,
        Command::Openapi => {
            println!("{}", serde_json::to_string_pretty(&openapi::document())?);
            Ok(())
        }
    }
}

/// Runs one service, or every one of them in this process.
async fn serve(which: Option<Service>) -> Result<()> {
    let config = Config::from_env()?;
    match which {
        Some(service) => serve_one(&config, service).await,
        None => serve_all(&config).await,
    }
}

/// One container per service: this process is `service` and nothing else.
async fn serve_one(config: &Config, service: Service) -> Result<()> {
    // Seeding the demo needs every schema at once, which a process owning one
    // of them does not have. Refused rather than ignored: a flag that is
    // silently dropped is an installation that is not what its compose file
    // says it is.
    if demo::requested() {
        anyhow::bail!("AUSTERIS_DEMO is honoured by `austeris serve` running every service; with a container per service, run `austeris demo` once instead");
    }

    let mut tasks = JoinSet::new();
    let peers = Peers::from_env();
    let app = match service {
        Service::Gateway => gateway::router(&peers),
        data => {
            let pool = schema_ready(config, data).await?;
            if data == Service::Identity {
                welcome_first_user(&pool).await?;
            }
            // A service that owns a schema also serves gRPC to its peers, on a
            // second port. The two are separate listeners rather than one
            // multiplexed port: the REST surface is what the gateway forwards
            // to, the gRPC one is what other services call, and only the first
            // should ever be reachable from outside the compose network.
            let grpc = service::grpc_bind();
            let grpc = TcpIncoming::bind(grpc.parse().context("AUSTERIS_GRPC_BIND is not an address")?)
                .map_err(|error| anyhow::anyhow!("binding gRPC on {grpc}: {error}"))?;
            start(data, pool, grpc, &peers, &mut tasks)?
        }
    };

    let listener = TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {} (AUSTERIS_BIND)", config.bind))?;
    tracing::info!(service = service.as_str(), address = %listener.local_addr()?, "listening");
    tasks.spawn(http(listener, app));
    first_to_stop(tasks).await
}

/// Every service in this one process, behind the gateway on the public port.
///
/// Each service keeps its own pool, its own schema and its own listeners - on
/// loopback ports chosen by the system, so nothing here can collide with
/// anything else on the machine. The gateway reaches them over those ports
/// exactly as it reaches containers over the compose network: one way of
/// talking, whichever shape the installation has (ADR 0008).
async fn serve_all(config: &Config) -> Result<()> {
    // Every schema first, and only then any listener: a gateway that answers
    // before its services have migrated would forward to half an installation.
    let mut pools = Vec::new();
    for &service in Service::routed() {
        pools.push((service, schema_ready(config, service).await?));
    }
    let pool_of = |wanted: Service| pools.iter().find(|(service, _)| *service == wanted).map(|(_, pool)| pool.clone());
    let (Some(identity), Some(ledger), Some(market)) = (pool_of(Service::Identity), pool_of(Service::Ledger), pool_of(Service::Market)) else {
        anyhow::bail!("a routed service has no pool; Service::routed() and this function disagree");
    };

    if demo::requested() {
        demo::seed(&demo::Books { identity, ledger, market }).await?.report();
    } else {
        welcome_first_user(&identity).await?;
    }

    // Every listener is bound before any service starts: a service that talks
    // to a peer - the market handing rates to the ledger - needs the peer's
    // address, and in one process that exists only once the port is bound.
    let loopback = std::net::Ipv4Addr::LOCALHOST;
    let mut listeners = Vec::new();
    let mut bound = Vec::new();
    for (service, pool) in pools {
        let rest = TcpListener::bind((loopback, 0)).await?;
        let grpc = TcpListener::bind((loopback, 0)).await?;
        bound.push((service, rest.local_addr()?, grpc.local_addr()?));
        listeners.push((service, pool, rest, grpc));
    }
    let peers = Peers::local(bound);

    let mut tasks = JoinSet::new();
    for (service, pool, rest, grpc) in listeners {
        let app = start(service, pool, TcpIncoming::from(grpc), &peers, &mut tasks)?;
        tasks.spawn(http(rest, app));
    }

    let listener = TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {} (AUSTERIS_BIND)", config.bind))?;
    tracing::info!(
        services = Service::routed().iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
        address = %listener.local_addr()?,
        "listening"
    );
    tasks.spawn(http(listener, gateway::router(&peers)));
    first_to_stop(tasks).await
}

/// Starts a service's gRPC listener as a task and hands back its REST router.
fn start(service: Service, pool: sqlx::PgPool, grpc: TcpIncoming, peers: &Peers, tasks: &mut JoinSet<Result<()>>) -> Result<Router> {
    // Each service's gRPC server is a different concrete type, so each branch
    // spawns its own task rather than handing back a future.
    match service {
        Service::Identity => {
            let server = Server::builder()
                .add_service(austeris_identity::grpc::Service::new(pool.clone()))
                .serve_with_incoming(grpc);
            tasks.spawn(async move { server.await.context("the identity gRPC listener stopped") });
            Ok(austeris_identity::routes::router(pool, secure_cookies()))
        }
        Service::Ledger => {
            let server = Server::builder()
                .add_service(austeris_ledger::grpc::Service::new(pool.clone()))
                .serve_with_incoming(grpc);
            tasks.spawn(async move { server.await.context("the ledger gRPC listener stopped") });
            Ok(austeris_ledger::routes::router(pool))
        }
        Service::Market => {
            let sources = std::sync::Arc::new(austeris_market::refresh::Sources::from_env());
            // Said once, at startup, rather than on every refresh: a source
            // without its key is switched off, not broken.
            tracing::info!(sources = sources.available().join(", "), "price sources available");
            let ledger = austeris_market::refresh::Ledger::connect_lazy(peers.grpc(Service::Ledger))?;

            let server = Server::builder()
                .add_service(austeris_market::grpc::Service::new(pool.clone()))
                .serve_with_incoming(grpc);
            tasks.spawn(async move { server.await.context("the market gRPC listener stopped") });
            // The hourly refresh never returns; wrapped so the task set, which
            // stops the process when any task ends, has a result type to hold.
            let refresher = austeris_market::refresh::every_hour(pool.clone(), sources.clone(), Some(ledger.clone()));
            tasks.spawn(async move {
                refresher.await;
                anyhow::bail!("the price refresher stopped")
            });
            Ok(austeris_market::routes::router(pool, sources, Some(ledger)))
        }
        Service::Gateway => unreachable!("the gateway owns no schema and serves no gRPC"),
    }
}

/// Serves a router on a listener until it stops.
async fn http(listener: TcpListener, app: Router) -> Result<()> {
    // `into_make_service_with_connect_info` rather than the plain one: the
    // gateway's rate limiter needs the peer address, and without this the
    // extractor has nothing to read.
    let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, app).await.context("an HTTP listener stopped")
}

/// Waits for the first listener to stop, and stops the process with it.
///
/// Any one stopping means the installation is no longer whole, so whichever
/// finishes first ends the process rather than leaving the rest answering for
/// a service that is gone - a restart policy can bring back a process, not
/// half of one.
async fn first_to_stop(mut tasks: JoinSet<Result<()>>) -> Result<()> {
    match tasks.join_next().await {
        Some(Ok(Ok(()))) => anyhow::bail!("a listener stopped without an error"),
        Some(Ok(Err(error))) => Err(error),
        Some(Err(error)) => Err(anyhow::Error::new(error).context("a listener panicked")),
        None => anyhow::bail!("nothing was started"),
    }
}

/// Creates the first account on an installation that has none, and says so.
async fn welcome_first_user(pool: &sqlx::PgPool) -> Result<()> {
    if let Some(password) = austeris_identity::routes::ensure_first_user(pool, &first_user_email()).await? {
        announce_first_user(&first_user_email(), &password);
    }
    Ok(())
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
