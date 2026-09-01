//! The market service: what can be priced, and what it was worth when.
//!
//! It owns the `market` schema and knows nothing about who holds an instrument
//! or what it cost them - that is the portfolio's business (ADR 0001). This
//! service answers one kind of question: the price of an instrument, now or at
//! a past instant.

pub mod grpc;
pub mod model;
pub mod repository;
pub mod routes;
pub mod source;

use sqlx::migrate::Migrator;

/// The schema this service owns.
pub const SCHEMA: &str = "market";

/// This service's migrations, embedded in the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
