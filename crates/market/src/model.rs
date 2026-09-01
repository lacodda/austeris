//! What this service stores.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of thing an instrument is.
///
/// Only `Crypto` has a source behind it today; the rest exist because the
/// column does, and adding a variant later would be a schema change in the
/// middle of a release that is about something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "instrument_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A cryptocurrency.
    Crypto,
    /// A share.
    Stock,
    /// A bond.
    Bond,
    /// A fund or ETF.
    Fund,
    /// A currency pair, for converting between the currencies in use.
    Fx,
    /// Something priced by hand: a flat, a car, a painting.
    Manual,
}

/// Something that can be priced.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Instrument {
    /// Stable identifier, used by every other service.
    pub id: Uuid,
    /// What kind of thing this is.
    pub kind: Kind,
    /// Ticker as the world writes it.
    pub symbol: String,
    /// Human-readable name.
    pub name: String,
    /// Smallest unit it trades in, when it has one.
    pub decimals: Option<i32>,
}

/// A price, as observed by one source at one instant.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Price {
    /// What was priced.
    pub instrument_id: Uuid,
    /// What it is quoted in - `USD`, `PYG`, `RUB`.
    pub quote_currency: String,
    /// When the source observed it, not when it was stored.
    pub observed_at: DateTime<Utc>,
    /// The price itself. Serialized as a string: a JSON number reaching a
    /// browser is an IEEE double, and the value is lost before it renders
    /// (ADR 0004).
    #[serde(with = "rust_decimal::serde::str")]
    pub price: Decimal,
    /// Which source said so.
    pub source: String,
}

/// A source's own name for an instrument, and where it sits in the order.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SourceBinding {
    /// Which instrument.
    pub instrument_id: Uuid,
    /// Which source.
    pub source: String,
    /// The identifier that source uses: a `CoinMarketCap` numeric id, a
    /// `CoinGecko` slug. Opaque to everything but the source itself.
    pub external_id: String,
    /// Lower wins.
    pub priority: i32,
}

/// A price a source just reported, before it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// The source's own identifier for the instrument.
    pub external_id: String,
    /// What the price is quoted in.
    pub quote_currency: String,
    /// The price.
    pub price: Decimal,
    /// When the source says it observed it.
    pub observed_at: DateTime<Utc>,
}
