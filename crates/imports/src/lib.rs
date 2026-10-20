//! The imports module: documents read into the books.
//!
//! A person's money arrives as paper and files long before it arrives as
//! entries: a receipt e-mailed by the shop, an exchange office's slip, later a
//! bank's statement. This module reads them and says what they were in the
//! ledger's terms - an entry posted through `ledger.v1.PostEntry` like any
//! module's (ADR 0001), with the file kept on it - so typing them in is not the
//! price of having books.
//!
//! What cannot be decided from a document alone - which account paid, when the
//! receipt does not say; what it was for, when the shop is new - is asked
//! rather than guessed: the document waits, with what it needs, until the
//! person says (ADR 0014).
//!
//! A module like any other, it can be switched off: `AUSTERIS_IMPORTS=off`.

pub mod book;
pub mod inbox;
pub mod ledger;
pub mod model;
pub mod receive;
pub mod repository;
pub mod routes;
pub mod sifen;
pub mod slip;

use sqlx::migrate::Migrator;

/// The schema this service owns.
pub const SCHEMA: &str = "imports";

/// This service's migrations, embedded in the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The name it signs the entries it posts with, so their history says who.
pub const SOURCE: &str = "imports";
