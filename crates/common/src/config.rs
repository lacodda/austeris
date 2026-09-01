//! Configuration read from the environment.
//!
//! Every variable is prefixed `AUSTERIS_`, and nothing loads a `.env` file: the
//! compose file owns the environment (ADR 0002). A binary that silently picks
//! up a stray `.env` from the working directory is a binary whose behaviour
//! depends on where it was started from.

use std::env::{self, VarError};
use std::time::Duration;

/// The `AUSTERIS_` prefix on every variable this reads.
const PREFIX: &str = "AUSTERIS_";

/// Settings shared by every service.
#[derive(Debug, Clone)]
pub struct Config {
    /// PostgreSQL connection string, or `None` when the environment does not
    /// supply one. The schema is selected separately, through `search_path` —
    /// see [`crate::db::connect`]. A service that owns a schema demands this
    /// with [`Config::database_url`]; the gateway, which owns none, starts
    /// without it.
    pub database_url: Option<String>,
    /// Address the service listens on.
    pub bind: String,
    /// Upper bound on pooled connections. Several services share one PostgreSQL
    /// on a Raspberry Pi, so each takes a small slice rather than the default.
    pub max_connections: u32,
    /// How long a request waits for a free connection before failing.
    pub acquire_timeout: Duration,
}

impl Config {
    /// Reads the configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a required variable is missing or when a
    /// numeric one does not parse.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            database_url: optional("DATABASE_URL"),
            bind: optional("BIND").unwrap_or_else(|| "0.0.0.0:8080".to_owned()),
            max_connections: parsed("MAX_CONNECTIONS", 5)?,
            acquire_timeout: Duration::from_secs(parsed("ACQUIRE_TIMEOUT_SECS", 30)?),
        })
    }

    /// The connection string, for a service that cannot run without one.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the variable is not set.
    pub fn database_url(&self) -> Result<&str, ConfigError> {
        self.database_url.as_deref().ok_or(ConfigError::Missing("DATABASE_URL"))
    }
}

/// Something the environment did not supply, or supplied unusably.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A variable without a default was not set.
    #[error("{PREFIX}{0} is not set")]
    Missing(&'static str),
    /// A variable was set to something that is not a number.
    #[error("{PREFIX}{name} is not a number: {value}")]
    NotANumber {
        /// The variable, without its prefix.
        name: &'static str,
        /// What it was set to.
        value: String,
    },
}

fn optional(name: &str) -> Option<String> {
    match env::var(format!("{PREFIX}{name}")) {
        Ok(value) if !value.is_empty() => Some(value),
        // A variable holding invalid Unicode is as absent as one never set: the
        // caller gets the default or a "not set" error, either way not a panic.
        Ok(_) | Err(VarError::NotPresent | VarError::NotUnicode(_)) => None,
    }
}

fn parsed<T: std::str::FromStr>(name: &'static str, default: T) -> Result<T, ConfigError> {
    match optional(name) {
        None => Ok(default),
        Some(value) => value.parse().map_err(|_| ConfigError::NotANumber { name, value }),
    }
}
