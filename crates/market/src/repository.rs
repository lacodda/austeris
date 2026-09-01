//! Reading and writing this service's schema.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Instrument, Kind, Price, SourceBinding};

/// Every instrument, newest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn instruments(pool: &PgPool) -> Result<Vec<Instrument>> {
    sqlx::query_as("SELECT id, kind, symbol, name, decimals FROM instruments ORDER BY kind, upper(symbol)")
        .fetch_all(pool)
        .await
        .context("listing instruments")
}

/// One instrument by id.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn instrument(pool: &PgPool, id: Uuid) -> Result<Option<Instrument>> {
    sqlx::query_as("SELECT id, kind, symbol, name, decimals FROM instruments WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading an instrument")
}

/// Creates an instrument, or returns the one already there.
///
/// Idempotent on purpose: a sync that runs twice must not create a second BTC,
/// and an operator adding one by hand should not have to check first.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn upsert_instrument(pool: &PgPool, kind: Kind, symbol: &str, name: &str, decimals: Option<i32>) -> Result<Instrument> {
    sqlx::query_as(
        "INSERT INTO instruments (kind, symbol, name, decimals) VALUES ($1, $2, $3, $4)
         ON CONFLICT (kind, upper(symbol))
         DO UPDATE SET name = EXCLUDED.name, decimals = EXCLUDED.decimals, updated_at = now()
         RETURNING id, kind, symbol, name, decimals",
    )
    .bind(kind)
    .bind(symbol)
    .bind(name)
    .bind(decimals)
    .fetch_one(pool)
    .await
    .context("creating an instrument")
}

/// Points a source at an instrument, replacing any earlier binding to it.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn bind_source(pool: &PgPool, instrument_id: Uuid, source: &str, external_id: &str, priority: i32) -> Result<()> {
    sqlx::query(
        "INSERT INTO instrument_sources (instrument_id, source, external_id, priority) VALUES ($1, $2, $3, $4)
         ON CONFLICT (instrument_id, source) DO UPDATE SET external_id = EXCLUDED.external_id, priority = EXCLUDED.priority",
    )
    .bind(instrument_id)
    .bind(source)
    .bind(external_id)
    .bind(priority)
    .execute(pool)
    .await
    .context("binding a source to an instrument")?;
    Ok(())
}

/// Every binding a source holds, so it can be asked for all of them at once.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn bindings_for(pool: &PgPool, source: &str) -> Result<Vec<SourceBinding>> {
    sqlx::query_as("SELECT instrument_id, source, external_id, priority FROM instrument_sources WHERE source = $1")
        .bind(source)
        .fetch_all(pool)
        .await
        .context("listing a source's instruments")
}

/// Records an observed price.
///
/// Two sources may report the same instant; both rows are kept, because which
/// one is preferred is a question about priority, answered at read time.
///
/// # Errors
///
/// Returns an error when the insert fails.
pub async fn record_price(
    pool: &PgPool,
    instrument_id: Uuid,
    quote_currency: &str,
    observed_at: DateTime<Utc>,
    price: rust_decimal::Decimal,
    source: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO prices (instrument_id, quote_currency, observed_at, price, source) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (instrument_id, quote_currency, observed_at, source) DO UPDATE SET price = EXCLUDED.price",
    )
    .bind(instrument_id)
    .bind(quote_currency)
    .bind(observed_at)
    .bind(price)
    .bind(source)
    .execute(pool)
    .await
    .context("recording a price")?;
    Ok(())
}

/// The price of an instrument at an instant, from the highest-priority source
/// that has one.
///
/// "At" means "as of": the newest observation not after the instant asked
/// about. A price is a fact about a moment that has already happened, so
/// answering with a later one would be answering a different question.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn price_at(pool: &PgPool, instrument_id: Uuid, quote_currency: &str, at: DateTime<Utc>) -> Result<Option<Price>> {
    // The join to `instrument_sources` is what makes priority mean something:
    // when two sources have a price, the one the operator ranked first wins.
    // `LEFT JOIN` so a price from a source no longer bound is still readable -
    // history must not vanish because a binding was removed today.
    sqlx::query_as(
        "SELECT p.instrument_id, p.quote_currency, p.observed_at, p.price, p.source
         FROM prices p
         LEFT JOIN instrument_sources s ON s.instrument_id = p.instrument_id AND s.source = p.source
         WHERE p.instrument_id = $1 AND p.quote_currency = $2 AND p.observed_at <= $3
         ORDER BY p.observed_at DESC, COALESCE(s.priority, 2147483647) ASC, p.source ASC
         LIMIT 1",
    )
    .bind(instrument_id)
    .bind(quote_currency)
    .bind(at)
    .fetch_optional(pool)
    .await
    .context("reading a price")
}

/// The latest price of each of several instruments, in one query.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn latest_prices(pool: &PgPool, instrument_ids: &[Uuid], quote_currency: &str) -> Result<Vec<Price>> {
    // `DISTINCT ON` keeps the first row per instrument in the given order,
    // which is the newest observation from the best-ranked source - the same
    // rule `price_at` applies, expressed once for a batch.
    sqlx::query_as(
        "SELECT DISTINCT ON (p.instrument_id)
                p.instrument_id, p.quote_currency, p.observed_at, p.price, p.source
         FROM prices p
         LEFT JOIN instrument_sources s ON s.instrument_id = p.instrument_id AND s.source = p.source
         WHERE p.instrument_id = ANY($1) AND p.quote_currency = $2
         ORDER BY p.instrument_id, p.observed_at DESC, COALESCE(s.priority, 2147483647) ASC, p.source ASC",
    )
    .bind(instrument_ids)
    .bind(quote_currency)
    .fetch_all(pool)
    .await
    .context("reading the latest prices")
}

/// Every observation for an instrument in a window, oldest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn price_history(pool: &PgPool, instrument_id: Uuid, quote_currency: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Price>> {
    sqlx::query_as(
        "SELECT instrument_id, quote_currency, observed_at, price, source
         FROM prices
         WHERE instrument_id = $1 AND quote_currency = $2 AND observed_at >= $3 AND observed_at <= $4
         ORDER BY observed_at ASC, source ASC",
    )
    .bind(instrument_id)
    .bind(quote_currency)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .context("reading a price history")
}
