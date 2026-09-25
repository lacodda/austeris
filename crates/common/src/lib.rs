//! Plumbing every austeris service shares: its configuration, its database
//! pool, the shape of its errors, and the two endpoints an orchestrator polls.
//!
//! Nothing here knows about accounts, prices or portfolios. A module that needs
//! another service's data asks that service over gRPC (ADR 0001); this crate
//! only makes the asking possible.

pub mod caller;
pub mod config;
pub mod db;
pub mod error;
pub mod freshness;
pub mod health;
pub mod http;
pub mod migrate;
pub mod telemetry;

pub use caller::{Caller, USER_HEADER};
pub use config::Config;
pub use error::{AppError, AppResult};
