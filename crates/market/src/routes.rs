//! The market service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards `/api/v1/market/...`
//! here (ADR 0001).

use std::sync::Arc;

use austeris_common::{AppError, AppResult, health};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Instrument, Kind, Price};
use crate::source::{CoinMarketCap, PriceSource};
use crate::{MIGRATOR, repository};

/// What every handler here needs.
#[derive(Clone)]
pub struct ServiceState {
    pool: PgPool,
    sources: Arc<Sources>,
}

/// The sources this service can ask, in the order it asks them.
pub struct Sources {
    coinmarketcap: CoinMarketCap,
}

impl Sources {
    /// Builds the set from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            coinmarketcap: CoinMarketCap::from_env(),
        }
    }

    /// Names the sources that are switched on.
    #[must_use]
    pub fn available(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.coinmarketcap.is_available() {
            names.push(self.coinmarketcap.name());
        }
        names
    }
}

/// Builds the service's router.
pub fn router(pool: PgPool, sources: Sources) -> Router {
    let state = ServiceState {
        pool: pool.clone(),
        sources: Arc::new(sources),
    };

    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool, &MIGRATOR))))
        .route("/market/instruments", get(list_instruments).post(create_instrument))
        .route("/market/instruments/sync", post(sync_instruments))
        .route("/market/instruments/{id}", get(read_instrument))
        .route("/market/instruments/{id}/sources", post(bind_source))
        .route("/market/prices", get(latest_prices))
        .route("/market/prices/{id}/history", get(price_history))
        .route("/market/prices/refresh", post(refresh_prices))
        .with_state(state)
}

/// What creating an instrument carries.
#[derive(Debug, Deserialize)]
pub struct NewInstrument {
    /// What kind of thing it is.
    pub kind: Kind,
    /// Ticker as the world writes it.
    pub symbol: String,
    /// Human-readable name.
    pub name: String,
    /// Smallest unit it trades in, when it has one.
    pub decimals: Option<i32>,
}

/// What binding a source carries.
#[derive(Debug, Deserialize)]
pub struct NewBinding {
    /// Which source, by the name it records prices under.
    pub source: String,
    /// That source's own identifier for this instrument.
    pub external_id: String,
    /// Lower wins. Defaults behind anything explicitly ranked.
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    100
}

async fn list_instruments(State(state): State<ServiceState>) -> AppResult<Json<Vec<Instrument>>> {
    Ok(Json(repository::instruments(&state.pool).await?))
}

async fn read_instrument(State(state): State<ServiceState>, Path(id): Path<Uuid>) -> AppResult<Json<Instrument>> {
    repository::instrument(&state.pool, id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such instrument")))
}

async fn create_instrument(State(state): State<ServiceState>, Json(new): Json<NewInstrument>) -> AppResult<(StatusCode, Json<Instrument>)> {
    if new.symbol.trim().is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("an instrument needs a symbol")));
    }

    let instrument = repository::upsert_instrument(&state.pool, new.kind, new.symbol.trim(), new.name.trim(), new.decimals).await?;
    Ok((StatusCode::CREATED, Json(instrument)))
}

async fn bind_source(State(state): State<ServiceState>, Path(id): Path<Uuid>, Json(new): Json<NewBinding>) -> AppResult<StatusCode> {
    // Binding a source to an instrument that does not exist would be accepted
    // by the foreign key and rejected as a 500; saying so is better.
    if repository::instrument(&state.pool, id).await?.is_none() {
        return Err(AppError::not_found(anyhow::anyhow!("no such instrument")));
    }

    repository::bind_source(&state.pool, id, &new.source, &new.external_id, new.priority).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Which instruments and currency a price query is about.
#[derive(Debug, Deserialize)]
pub struct PriceQuery {
    /// Comma-separated instrument ids. Every instrument when omitted.
    pub instruments: Option<String>,
    /// Currency code. USD when omitted.
    #[serde(default = "default_currency")]
    pub currency: String,
}

fn default_currency() -> String {
    "USD".to_owned()
}

async fn latest_prices(State(state): State<ServiceState>, Query(query): Query<PriceQuery>) -> AppResult<Json<Vec<Price>>> {
    let ids = match &query.instruments {
        Some(list) => list
            .split(',')
            .map(|id| {
                id.trim()
                    .parse::<Uuid>()
                    .map_err(|_| AppError::bad_request(anyhow::anyhow!("`{id}` is not an instrument id")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => repository::instruments(&state.pool).await?.into_iter().map(|i| i.id).collect(),
    };

    Ok(Json(repository::latest_prices(&state.pool, &ids, &query.currency).await?))
}

/// The window a history is asked for.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Currency code. USD when omitted.
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Start of the window. Thirty days back when omitted.
    pub from: Option<DateTime<Utc>>,
    /// End of the window. Now when omitted.
    pub to: Option<DateTime<Utc>>,
}

async fn price_history(State(state): State<ServiceState>, Path(id): Path<Uuid>, Query(query): Query<HistoryQuery>) -> AppResult<Json<Vec<Price>>> {
    let to = query.to.unwrap_or_else(Utc::now);
    let from = query.from.unwrap_or(to - Duration::days(30));

    if from > to {
        return Err(AppError::bad_request(anyhow::anyhow!("the window ends before it starts")));
    }

    Ok(Json(repository::price_history(&state.pool, id, &query.currency, from, to).await?))
}

/// How much of a source's catalogue to take.
#[derive(Debug, Deserialize)]
pub struct SyncQuery {
    /// How many instruments to import, most prominent first. A hundred when
    /// omitted - enough to cover what a person actually holds without pulling
    /// ten thousand rows nobody will ever price.
    #[serde(default = "default_sync_limit")]
    pub limit: usize,
}

fn default_sync_limit() -> usize {
    100
}

/// What a sync did.
#[derive(Debug, Serialize)]
pub struct Synced {
    /// How many instruments exist after it.
    pub instruments: usize,
    /// Which source was read.
    pub source: String,
}

/// Imports a source's catalogue as instruments, binding each to that source.
///
/// Idempotent: running it twice updates names and leaves ids alone, so nothing
/// that references an instrument breaks.
async fn sync_instruments(State(state): State<ServiceState>, Query(query): Query<SyncQuery>) -> AppResult<Json<Synced>> {
    let source = &state.sources.coinmarketcap;
    if !source.is_available() {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            anyhow::anyhow!("the {} source is switched off; set AUSTERIS_CMC_API_KEY", source.name()),
        ));
    }

    let listings = source.catalogue(query.limit).await.map_err(AppError::internal)?;

    let mut imported = 0;
    for listing in listings {
        let instrument = repository::upsert_instrument(&state.pool, Kind::Crypto, &listing.symbol, &listing.name, None).await?;
        // Priority 100 - the default rank. An operator who wants this source
        // preferred moves it explicitly.
        repository::bind_source(&state.pool, instrument.id, source.name(), &listing.external_id, 100).await?;
        imported += 1;
    }

    Ok(Json(Synced {
        instruments: imported,
        source: source.name().to_owned(),
    }))
}

/// What a refresh did.
#[derive(Debug, Serialize)]
pub struct Refreshed {
    /// How many prices were recorded.
    pub recorded: usize,
    /// Which sources answered.
    pub sources: Vec<String>,
    /// Sources that were asked and failed - a partial refresh reports rather
    /// than pretends.
    pub failed: Vec<String>,
}

/// Asks every available source for the instruments bound to it.
///
/// Sources are independent: one failing costs its own instruments' prices and
/// nothing else. In 2025 there was one source, it went down, and prices simply
/// stopped accruing with nothing saying so.
async fn refresh_prices(State(state): State<ServiceState>) -> AppResult<Json<Refreshed>> {
    let mut recorded = 0;
    let mut answered = Vec::new();
    let mut failed = Vec::new();

    for source in state.sources.available() {
        let bindings = repository::bindings_for(&state.pool, source).await?;
        if bindings.is_empty() {
            continue;
        }

        let external_ids: Vec<String> = bindings.iter().map(|b| b.external_id.clone()).collect();
        let quotes = match state.sources.coinmarketcap.quotes(&external_ids).await {
            Ok(quotes) => quotes,
            Err(error) => {
                tracing::error!(source, %error, "a price source failed");
                failed.push(source.to_owned());
                continue;
            }
        };

        for quote in quotes {
            let Some(binding) = bindings.iter().find(|b| b.external_id == quote.external_id) else {
                // The source answered about something nobody asked for. Not an
                // error, but not ours to store either.
                continue;
            };
            repository::record_price(
                &state.pool,
                binding.instrument_id,
                &quote.quote_currency,
                quote.observed_at,
                quote.price,
                source,
            )
            .await?;
            recorded += 1;
        }

        answered.push(source.to_owned());
    }

    Ok(Json(Refreshed {
        recorded,
        sources: answered,
        failed,
    }))
}
