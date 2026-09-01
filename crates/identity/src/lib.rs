//! The identity service: who uses this installation, and how they stay signed in.
//!
//! It owns the `identity` schema and is the only place a password is ever seen.
//! Other services never learn one: they ask this service whether a session is
//! good, and get back a user id (ADR 0001).

pub mod grpc;
pub mod password;
pub mod routes;
pub mod session;

use sqlx::migrate::Migrator;

/// The schema this service owns.
pub const SCHEMA: &str = "identity";

/// This service's migrations, embedded in the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
