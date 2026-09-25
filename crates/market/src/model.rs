//! What this service stores.

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of thing an instrument is.
///
/// `Crypto` and `Fx` have sources behind them today; the rest exist because
/// the column does, and adding a variant later would be a schema change in the
/// middle of a release that is about something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
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

impl Kind {
    /// Whether a price observed at `observed_at` is no longer current at `at`.
    ///
    /// Crypto trades every hour of every day, so a price two refreshes old has
    /// missed movement that matters. Currencies, shares and funds are priced on
    /// business days, so they go stale on the same calendar as the ledger's
    /// rates. Something priced by hand is as current as its owner says.
    #[must_use]
    pub fn is_stale(self, observed_at: DateTime<Utc>, at: DateTime<Utc>) -> bool {
        match self {
            Self::Crypto => at - observed_at > Duration::hours(2),
            Self::Manual => false,
            Self::Stock | Self::Bond | Self::Fund | Self::Fx => {
                (at.date_naive() - observed_at.date_naive()).num_days() > austeris_common::freshness::DAILY_RATE_DAYS
            }
        }
    }
}

/// Something that can be priced.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
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
    #[schema(value_type = String, example = "61234.567890123456789000")]
    pub price: Decimal,
    /// Which source said so.
    pub source: String,
}

/// The latest price of an instrument, and whether it is still current.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LatestPrice {
    /// The price as it was recorded.
    #[serde(flatten)]
    pub price: Price,
    /// Older than its kind stays current for: two hours for crypto, a long
    /// weekend for anything priced on business days. The number is still the
    /// last one known; this is what stops it being read as now.
    pub stale: bool,
}

/// Whether a source is answering, and how fresh what it last said is.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SourceStatus {
    /// The name its prices are recorded under.
    pub name: String,
    /// What it prices.
    pub prices: Kind,
    /// Whether it is switched on.
    pub available: bool,
    /// Why it is switched off, when it is: the setting it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_because: Option<String>,
    /// The newest observation it has recorded, whether or not it is on now.
    pub last_observed_at: Option<DateTime<Utc>>,
    /// Nothing recorded yet, or the newest is older than its kind stays current
    /// for. A switched-off source is stale by its second hour: its prices are
    /// still shown, and this is what says they are old.
    pub stale: bool,
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

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::Kind;

    #[test]
    fn crypto_goes_stale_within_hours_and_currencies_after_a_long_weekend() {
        let friday = Utc.with_ymd_and_hms(2026, 9, 25, 0, 0, 0).single().expect("an instant");

        assert!(!Kind::Crypto.is_stale(friday, friday + Duration::hours(2)));
        assert!(Kind::Crypto.is_stale(friday, friday + Duration::hours(3)));

        // Friday's table is current through Tuesday after a Monday holiday...
        assert!(!Kind::Fx.is_stale(friday, friday + Duration::days(4) + Duration::hours(23)));
        // ...and not on Wednesday.
        assert!(Kind::Fx.is_stale(friday, friday + Duration::days(5)));

        assert!(!Kind::Manual.is_stale(friday, friday + Duration::days(400)));
    }
}
