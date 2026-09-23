//! Which services exist, what each owns, and where the gateway forwards them.
//!
//! This is the single place a new service is registered. Adding one means a
//! variant here, a crate, a schema and a compose entry - the checklist from
//! ADR 0001, with this file as its first line.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::migrate::Migrator;

/// A service this binary can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Service {
    /// The only public surface: routes to the others and serves the web UI.
    Gateway,
    /// People, passwords and sessions.
    Identity,
    /// Accounts, categories and the movements between them.
    Ledger,
    /// Instruments and their prices.
    Market,
}

impl Service {
    /// Every service, in registration order.
    pub const ALL: &'static [Self] = &[Self::Gateway, Self::Identity, Self::Ledger, Self::Market];

    /// The service's name, as it appears in a command line and in a URL.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
            Self::Identity => "identity",
            Self::Ledger => "ledger",
            Self::Market => "market",
        }
    }

    /// The schema this service owns, or `None` when it owns none.
    ///
    /// The gateway owns none: it holds no data of its own, which is also why it
    /// starts on a machine with no database at all.
    #[must_use]
    pub fn schema(self) -> Option<&'static str> {
        match self {
            Self::Gateway => None,
            Self::Identity => Some(austeris_identity::SCHEMA),
            Self::Ledger => Some(austeris_ledger::SCHEMA),
            Self::Market => Some(austeris_market::SCHEMA),
        }
    }

    /// The migrations this service carries, if it owns a schema.
    #[must_use]
    pub fn migrator(self) -> Option<&'static Migrator> {
        match self {
            Self::Gateway => None,
            Self::Identity => Some(&austeris_identity::MIGRATOR),
            Self::Ledger => Some(&austeris_ledger::MIGRATOR),
            Self::Market => Some(&austeris_market::MIGRATOR),
        }
    }

    /// Services the gateway routes to, in the order they are matched.
    ///
    /// The gateway is not among them: it does not forward to itself.
    #[must_use]
    pub fn routed() -> &'static [Self] {
        &[Self::Identity, Self::Ledger, Self::Market]
    }

    /// The path prefix under `/api/v1` this service answers on.
    ///
    /// Usually its own name; `identity` is the exception because what a client
    /// calls is `auth`, and naming a URL after an internal service leaks the
    /// deployment's shape into a contract that has to outlive it.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Gateway => "",
            Self::Identity => "auth",
            Self::Ledger => "ledger",
            Self::Market => "market",
        }
    }
}

/// Where each routed service answers: REST for the gateway, gRPC for peers.
///
/// A table handed to the gateway rather than something each call looks up in
/// the environment, because the answer depends on how the installation runs.
/// One container per service reaches its peers by compose name; one process
/// running every service reaches them on the loopback ports it bound itself,
/// which exist only once the process is up and cannot be written down ahead of
/// time.
#[derive(Debug, Clone)]
pub struct Peers {
    endpoints: Arc<HashMap<Service, Endpoints>>,
}

#[derive(Debug)]
struct Endpoints {
    rest: String,
    grpc: String,
}

impl Peers {
    /// One container per service: the compose names, overridable per service.
    ///
    /// `AUSTERIS_IDENTITY_ADDR` and `AUSTERIS_IDENTITY_GRPC_ADDR` let a
    /// developer run one service outside compose while the rest stay in it.
    #[must_use]
    pub fn from_env() -> Self {
        let endpoints = Service::routed()
            .iter()
            .map(|&service| {
                let name = service.as_str();
                let upper = name.to_uppercase();
                let rest = std::env::var(format!("AUSTERIS_{upper}_ADDR")).unwrap_or_else(|_| format!("http://{name}:8080"));
                let grpc = std::env::var(format!("AUSTERIS_{upper}_GRPC_ADDR")).unwrap_or_else(|_| format!("http://{name}:9090"));
                (service, Endpoints { rest, grpc })
            })
            .collect();
        Self {
            endpoints: Arc::new(endpoints),
        }
    }

    /// Every service in this process, on the addresses its listeners bound.
    #[must_use]
    pub fn local(bound: impl IntoIterator<Item = (Service, SocketAddr, SocketAddr)>) -> Self {
        let endpoints = bound
            .into_iter()
            .map(|(service, rest, grpc)| {
                (
                    service,
                    Endpoints {
                        rest: format!("http://{rest}"),
                        grpc: format!("http://{grpc}"),
                    },
                )
            })
            .collect();
        Self {
            endpoints: Arc::new(endpoints),
        }
    }

    /// Where the gateway forwards this service's REST traffic.
    ///
    /// # Panics
    ///
    /// When the service is not in the table - a routed service missing from
    /// it is a wiring mistake in this binary, not a condition a request can
    /// cause.
    #[must_use]
    pub fn rest(&self, service: Service) -> &str {
        &self.endpoint(service).rest
    }

    /// Where this service's gRPC surface is reached.
    ///
    /// # Panics
    ///
    /// As [`Peers::rest`].
    #[must_use]
    pub fn grpc(&self, service: Service) -> &str {
        &self.endpoint(service).grpc
    }

    fn endpoint(&self, service: Service) -> &Endpoints {
        self.endpoints
            .get(&service)
            .unwrap_or_else(|| panic!("{service} has no address; every routed service must be given one"))
    }
}

/// The address a service serves gRPC on.
///
/// A second port, never published outside the compose network: this is the
/// surface peers use, and nothing outside is a peer. One setting for every
/// service, because each runs in its own container. A process running every
/// service ignores it and binds loopback ports of its own choosing.
#[must_use]
pub fn grpc_bind() -> String {
    std::env::var("AUSTERIS_GRPC_BIND").unwrap_or_else(|_| "0.0.0.0:9090".to_owned())
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{Peers, Service};

    #[test]
    fn every_service_is_listed_in_all() {
        // `ALL` drives `migrate` with no argument: a service missing from it
        // has its schema silently left behind at the previous version.
        for service in [Service::Gateway, Service::Identity, Service::Ledger, Service::Market] {
            assert!(Service::ALL.contains(&service), "{service} is missing from Service::ALL");
        }
    }

    #[test]
    fn a_service_owning_a_schema_carries_migrations_for_it() {
        for service in Service::ALL {
            assert_eq!(
                service.schema().is_some(),
                service.migrator().is_some(),
                "{service} has a schema without migrations, or the other way round"
            );
        }
    }

    #[test]
    fn every_routed_service_has_a_prefix_and_the_gateway_has_none() {
        assert!(Service::routed().iter().all(|s| !s.prefix().is_empty()));
        assert!(!Service::routed().contains(&Service::Gateway), "the gateway must not forward to itself");
        assert!(Service::Gateway.prefix().is_empty());
    }

    #[test]
    fn every_routed_service_has_an_address_in_either_shape() {
        let from_env = Peers::from_env();
        let loopback = std::net::SocketAddr::from(([127, 0, 0, 1], 1));
        let local = Peers::local(Service::routed().iter().map(|&s| (s, loopback, loopback)));
        for &service in Service::routed() {
            assert!(from_env.rest(service).starts_with("http://"));
            assert!(from_env.grpc(service).starts_with("http://"));
            assert_eq!(local.rest(service), "http://127.0.0.1:1");
        }
    }

    #[test]
    fn no_two_services_share_a_prefix_or_a_schema() {
        // Two services on one prefix means one of them is unreachable; two on
        // one schema means they read each other's tables (ADR 0001).
        let mut prefixes: Vec<_> = Service::routed().iter().map(|s| s.prefix()).collect();
        prefixes.sort_unstable();
        let count = prefixes.len();
        prefixes.dedup();
        assert_eq!(prefixes.len(), count, "two services answer on the same prefix");

        let mut schemas: Vec<_> = Service::ALL.iter().filter_map(|s| s.schema()).collect();
        schemas.sort_unstable();
        let count = schemas.len();
        schemas.dedup();
        assert_eq!(schemas.len(), count, "two services own the same schema");
    }
}
