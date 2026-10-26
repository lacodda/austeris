//! The portfolio module: what a person bought, sold and moved, what it adds up
//! to, and what it is worth.
//!
//! A trade happens on one of the person's accounts in the ledger - a broker's,
//! an exchange's - whose balance is the money there: dollars, or a stablecoin
//! kept as money. The trade's money is booked in the ledger as an entry this
//! module keeps (ADR 0015): the account moves, and the other side is the
//! holding the money became, which no report of spending counts. The coins and
//! shares are this module's: a position is what the trades add up to, valued
//! at the market's prices.
//!
//! A module like any other, it can be switched off: `AUSTERIS_PORTFOLIO=off`.

pub mod model;
pub mod repository;
pub mod routes;
pub mod services;
pub mod snapshot;
pub mod trade;
pub mod value;

use sqlx::migrate::Migrator;

/// The schema this service owns.
pub const SCHEMA: &str = "portfolio";

/// This service's migrations, embedded in the binary.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The name it signs the entries it posts and keeps with.
pub const SOURCE: &str = "portfolio";

/// The kinds of account positions are held on: a broker's and a wallet's,
/// whose balance is the money a trade there is paid in.
pub const HOLDING_KINDS: [&str; 2] = ["brokerage", "crypto_wallet"];
