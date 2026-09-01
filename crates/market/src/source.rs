//! Where prices come from.
//!
//! A source is anything that can be asked "what are these worth". They are
//! tried in priority order and the first that answers wins: in 2025 there was
//! one source, `CoinMarketCap` went down, and prices simply stopped accruing.
//!
//! No source is contacted in a test. The fixtures are recorded responses, so
//! the suite is not a client of anyone's rate limit and does not go red when a
//! third party has a bad afternoon.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::model::Quote;

/// Something that can be asked for prices.
#[allow(
    async_fn_in_trait,
    reason = "the only implementors are in this crate, and none of them is stored as a trait object"
)]
pub trait PriceSource {
    /// The name this source is recorded under, in `instrument_sources.source`
    /// and on every price row it produces.
    fn name(&self) -> &'static str;

    /// Whether this source can be used at all.
    ///
    /// A source without its credentials is not an error - it is switched off,
    /// and the service says so once at startup rather than failing every
    /// refresh.
    fn is_available(&self) -> bool;

    /// Asks for the current price of each identifier, in USD.
    ///
    /// The identifiers are this source's own, from `instrument_sources`.
    /// An identifier the source does not answer for is simply absent from the
    /// result - one unknown symbol must not lose the whole batch.
    async fn quotes(&self, external_ids: &[String]) -> Result<Vec<Quote>>;

    /// Lists the instruments this source knows about, most prominent first.
    ///
    /// Used to populate the catalogue rather than to price anything: an
    /// operator adding BTC should not have to look up its numeric id.
    async fn catalogue(&self, limit: usize) -> Result<Vec<Listing>>;
}

/// An instrument a source knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The source's own identifier.
    pub external_id: String,
    /// Ticker as the source writes it.
    pub symbol: String,
    /// Human-readable name.
    pub name: String,
}

/// `CoinMarketCap`.
pub struct CoinMarketCap {
    client: reqwest::Client,
    /// Absent when `AUSTERIS_CMC_API_KEY` is not set: the source is then off.
    api_key: Option<String>,
    /// Overridden by tests to point at a local server; the real base otherwise.
    base_url: String,
}

/// The name `CoinMarketCap` prices are recorded under.
pub const COINMARKETCAP: &str = "coinmarketcap";

impl CoinMarketCap {
    /// Reads the key from the environment. Missing is a valid state.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            client: reqwest::Client::new(),
            // Read once, never logged: a key in a log line is a key in whatever
            // that log is shipped to.
            api_key: std::env::var("AUSTERIS_CMC_API_KEY").ok().filter(|key| !key.is_empty()),
            base_url: "https://pro-api.coinmarketcap.com".to_owned(),
        }
    }

    /// Points the source at another base URL, for tests against a recorded
    /// response.
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Supplies a key without reading the environment, for tests.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// How many identifiers fit in one request. The API's own limit.
    const BATCH: usize = 100;
}

impl PriceSource for CoinMarketCap {
    fn name(&self) -> &'static str {
        COINMARKETCAP
    }

    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    async fn quotes(&self, external_ids: &[String]) -> Result<Vec<Quote>> {
        let Some(api_key) = &self.api_key else {
            anyhow::bail!("the CoinMarketCap source is switched off: AUSTERIS_CMC_API_KEY is not set");
        };

        let mut quotes = Vec::new();
        for chunk in external_ids.chunks(Self::BATCH) {
            let ids = chunk.join(",");
            let response = self
                .client
                .get(format!("{}/v2/cryptocurrency/quotes/latest", self.base_url))
                .header("X-CMC_PRO_API_KEY", api_key)
                .query(&[("id", ids.as_str()), ("convert", "USD")])
                .send()
                .await
                .context("asking CoinMarketCap for quotes")?;

            // The status line first: the body of an error response does not
            // parse as quotes, and "missing field `data`" is a worse message
            // than the status that caused it.
            let status = response.status();
            let body = response.text().await.context("reading CoinMarketCap's answer")?;
            anyhow::ensure!(status.is_success(), "CoinMarketCap answered {status}");

            quotes.extend(parse_quotes(&body).context("parsing CoinMarketCap's answer")?);
        }

        Ok(quotes)
    }

    async fn catalogue(&self, limit: usize) -> Result<Vec<Listing>> {
        let Some(api_key) = &self.api_key else {
            anyhow::bail!("the CoinMarketCap source is switched off: AUSTERIS_CMC_API_KEY is not set");
        };

        let response = self
            .client
            .get(format!("{}/v1/cryptocurrency/listings/latest", self.base_url))
            .header("X-CMC_PRO_API_KEY", api_key)
            .query(&[("start", "1"), ("limit", &limit.to_string()), ("convert", "USD")])
            .send()
            .await
            .context("asking CoinMarketCap for its catalogue")?;

        let status = response.status();
        let body = response.text().await.context("reading CoinMarketCap's answer")?;
        anyhow::ensure!(status.is_success(), "CoinMarketCap answered {status}");

        parse_listings(&body).context("parsing CoinMarketCap's catalogue")
    }
}

/// Turns a `listings/latest` body into listings.
fn parse_listings(body: &str) -> Result<Vec<Listing>> {
    let response: CmcListingsResponse = serde_json::from_str(body)?;

    Ok(response
        .data
        .into_iter()
        .map(|listing| Listing {
            external_id: listing.id.to_string(),
            symbol: listing.symbol,
            name: listing.name,
        })
        .collect())
}

/// Turns a `quotes/latest` body into quotes.
///
/// Separate from the request so it can be tested against a recorded response
/// without a server, and so a malformed entry can be dropped rather than
/// losing the batch.
fn parse_quotes(body: &str) -> Result<Vec<Quote>> {
    let response: CmcQuotesResponse = serde_json::from_str(body)?;

    Ok(response
        .data
        .into_iter()
        .filter_map(|(external_id, entry)| {
            let usd = entry.quote.get("USD")?;
            Some(Quote {
                external_id,
                quote_currency: "USD".to_owned(),
                price: usd.price?,
                observed_at: usd.last_updated,
            })
        })
        .collect())
}

/// The shape of `v2/cryptocurrency/quotes/latest`.
///
/// Only the fields this service uses are declared: the response carries several
/// dozen more, and every one named here is one that can change under us.
#[derive(Debug, Deserialize)]
struct CmcQuotesResponse {
    data: HashMap<String, CmcEntry>,
}

/// The shape of `v1/cryptocurrency/listings/latest`, trimmed the same way.
#[derive(Debug, Deserialize)]
struct CmcListingsResponse {
    data: Vec<CmcListing>,
}

#[derive(Debug, Deserialize)]
struct CmcListing {
    id: i64,
    name: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct CmcEntry {
    quote: HashMap<String, CmcQuote>,
}

#[derive(Debug, Deserialize)]
struct CmcQuote {
    /// Null for an instrument the source has no price for - which is a fact,
    /// not a parse failure.
    ///
    /// The attribute and `serde_json`'s `arbitrary_precision` feature are one
    /// mechanism, not two: the feature hands a number over as its digits rather
    /// than as an f64, and this reader is the one that accepts them in that
    /// form. Drop either and 61234.567890123456 becomes 61234.56789012346 - or
    /// stops parsing altogether. Both are covered by the tests below.
    #[serde(default, with = "rust_decimal::serde::arbitrary_precision_option")]
    price: Option<Decimal>,
    last_updated: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::{CoinMarketCap, PriceSource, parse_quotes};

    /// A recorded `quotes/latest` response, trimmed to the fields this service
    /// reads. Not captured from a live call with anyone's key.
    const RECORDED: &str = r#"{
      "status": {"error_code": 0, "error_message": null},
      "data": {
        "1": {"quote": {"USD": {"price": 61234.567890123456, "last_updated": "2026-09-01T12:00:00.000Z"}}},
        "1027": {"quote": {"USD": {"price": 2456.78, "last_updated": "2026-09-01T12:00:00.000Z"}}}
      }
    }"#;

    #[test]
    fn a_recorded_response_becomes_quotes() {
        let mut quotes = parse_quotes(RECORDED).expect("parsing");
        quotes.sort_by(|a, b| a.external_id.cmp(&b.external_id));

        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0].external_id, "1");
        assert_eq!(quotes[0].quote_currency, "USD");
        assert_eq!(quotes[0].price.to_string(), "61234.567890123456");
        assert_eq!(quotes[1].price.to_string(), "2456.78");
    }

    /// A price read through an f64, for the comparison below.
    #[derive(serde::Deserialize)]
    struct ViaFloat {
        price: f64,
    }

    #[test]
    fn the_price_keeps_every_digit_the_source_sent() {
        let quotes = parse_quotes(RECORDED).expect("parsing");
        let btc = quotes.iter().find(|q| q.external_id == "1").expect("the entry");
        assert_eq!(btc.price.to_string(), "61234.567890123456");

        // What the alternative costs, measured rather than asserted from
        // memory: the same JSON number read as an f64 loses its tail on the way
        // in, and no later care can put it back. This is ADR 0004 in one line -
        // and the reason the field is a Decimal rather than the f64 the 2025
        // schema stored.
        let lossy: ViaFloat = serde_json::from_str(r#"{"price": 61234.567890123456}"#).expect("parsing as a float");
        assert_ne!(
            lossy.price.to_string(),
            "61234.567890123456",
            "if an f64 could carry this value, the test above would prove nothing"
        );
    }

    #[test]
    fn an_instrument_without_a_price_is_absent_rather_than_fatal() {
        // CoinMarketCap sends `"price": null` for something it does not price.
        // Losing the whole batch over one of them is how a refresh silently
        // stops updating everything else.
        let body = r#"{"data": {
            "1": {"quote": {"USD": {"price": null, "last_updated": "2026-09-01T12:00:00.000Z"}}},
            "1027": {"quote": {"USD": {"price": 2456.78, "last_updated": "2026-09-01T12:00:00.000Z"}}}
        }}"#;

        let quotes = parse_quotes(body).expect("parsing");
        assert_eq!(quotes.len(), 1, "the priced instrument survived, the unpriced one did not appear");
        assert_eq!(quotes[0].external_id, "1027");
    }

    #[test]
    fn a_source_without_a_key_is_off_rather_than_broken() {
        // The service must start and say so, not fail every refresh.
        let source = CoinMarketCap {
            client: reqwest::Client::new(),
            api_key: None,
            base_url: String::new(),
        };
        assert!(!source.is_available());
        assert_eq!(source.name(), "coinmarketcap");
    }

    #[tokio::test]
    async fn asking_a_switched_off_source_says_why() {
        let source = CoinMarketCap {
            client: reqwest::Client::new(),
            api_key: None,
            base_url: String::new(),
        };
        let error = source.quotes(&["1".to_owned()]).await.expect_err("a source with no key");
        assert!(error.to_string().contains("AUSTERIS_CMC_API_KEY"), "{error}");
    }
}
