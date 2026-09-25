//! The ledger service: where money sits, what it is spent on, and every
//! movement between the two.
//!
//! This is the core the modules post into, and it knows nothing about any of
//! them (ADR 0001). There is not a line here about crypto, loans or salaries:
//! a module works out what happened in its own terms and hands the ledger an
//! entry, which is the only shape the ledger understands.
//!
//! That shape is double entry. An entry is a header and its lines, and the
//! lines sum to zero in every currency they touch - a rule the database holds
//! rather than the code that usually writes. Because of it, a plain expense, a
//! transfer between accounts, a receipt split across three categories and a
//! currency exchange are one mechanism rather than four, and no balance is ever
//! stored: an account's balance is the sum of its lines.

pub mod balance;
pub mod currency;
pub mod exchange;
pub mod grpc;
pub mod model;
pub mod parse;
pub mod repository;
pub mod routes;

use sqlx::migrate::Migrator;

/// The schema this service owns.
pub const SCHEMA: &str = "ledger";

/// This service's migrations, embedded in the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
