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
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Instrument, Kind, LatestPrice, Price, SourceStatus};
use crate::refresh::{self, Failure, Ledger, Refreshed, Sources};
use crate::source::PriceSource;
use crate::{MIGRATOR, repository};

/// What every handler here needs.
#[derive(Clone)]
pub struct ServiceState {
    pool: PgPool,
    sources: Arc<Sources>,
    ledger: Option<Ledger>,
}

/// Builds the service's router.
///
/// `ledger` is where official rates are handed on after a refresh; `None`
/// records them here only, which is what a test of this service alone wants.
pub fn router(pool: PgPool, sources: Arc<Sources>, ledger: Option<Ledger>) -> Router {
    let state = ServiceState {
        pool: pool.clone(),
        sources,
        ledger,
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
        .route("/market/sources", get(list_sources))
        .with_state(state)
}

/// What creating an instrument carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
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
#[derive(Debug, Deserialize, utoipa::ToSchema)]
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

#[utoipa::path(
    get,
    path = "/api/v1/market/instruments",
    tag = "market",
    responses((status = 200, description = "Everything that can be priced", body = Vec<Instrument>)),
)]
async fn list_instruments(State(state): State<ServiceState>) -> AppResult<Json<Vec<Instrument>>> {
    Ok(Json(repository::instruments(&state.pool).await?))
}

#[utoipa::path(
    get,
    path = "/api/v1/market/instruments/{id}",
    tag = "market",
    params(("id" = Uuid, Path, description = "The instrument")),
    responses(
        (status = 200, description = "The instrument", body = Instrument),
        (status = 404, description = "No such instrument", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_instrument(State(state): State<ServiceState>, Path(id): Path<Uuid>) -> AppResult<Json<Instrument>> {
    repository::instrument(&state.pool, id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such instrument")))
}

/// Creating one that already exists updates its name and returns the same id,
/// so a sync run twice does not produce two Bitcoins.
#[utoipa::path(
    post,
    path = "/api/v1/market/instruments",
    tag = "market",
    request_body = NewInstrument,
    responses(
        (status = 201, description = "The instrument, created or updated", body = Instrument),
        (status = 400, description = "An instrument needs a symbol", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_instrument(State(state): State<ServiceState>, Json(new): Json<NewInstrument>) -> AppResult<(StatusCode, Json<Instrument>)> {
    if new.symbol.trim().is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("an instrument needs a symbol")));
    }

    let instrument = repository::upsert_instrument(&state.pool, new.kind, new.symbol.trim(), new.name.trim(), new.decimals).await?;
    Ok((StatusCode::CREATED, Json(instrument)))
}

/// Points a source at an instrument. `priority` is a rank: lower wins.
#[utoipa::path(
    post,
    path = "/api/v1/market/instruments/{id}/sources",
    tag = "market",
    params(("id" = Uuid, Path, description = "The instrument")),
    request_body = NewBinding,
    responses(
        (status = 204, description = "Bound"),
        (status = 404, description = "No such instrument", body = austeris_common::error::ErrorBody),
    ),
)]
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
#[derive(Debug, Deserialize, utoipa::IntoParams)]
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

/// An instrument with no price is absent from the answer rather than an error:
/// one unpriced instrument must not cost a whole batch. A price older than its
/// kind stays current for is still answered - it is the last one known - and
/// marked `stale`, so it is not read as now.
#[utoipa::path(
    get,
    path = "/api/v1/market/prices",
    tag = "market",
    params(PriceQuery),
    responses((status = 200, description = "The latest price of each instrument, and whether it is still current", body = Vec<LatestPrice>)),
)]
async fn latest_prices(State(state): State<ServiceState>, Query(query): Query<PriceQuery>) -> AppResult<Json<Vec<LatestPrice>>> {
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

    let now = Utc::now();
    Ok(Json(
        repository::latest_prices(&state.pool, &ids, &query.currency)
            .await?
            .into_iter()
            .map(|(price, kind)| LatestPrice {
                stale: kind.is_stale(price.observed_at, now),
                price,
            })
            .collect(),
    ))
}

/// The window a history is asked for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct HistoryQuery {
    /// Currency code. USD when omitted.
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Start of the window. Thirty days back when omitted.
    pub from: Option<DateTime<Utc>>,
    /// End of the window. Now when omitted.
    pub to: Option<DateTime<Utc>>,
}

/// Every observation in a window, oldest first.
#[utoipa::path(
    get,
    path = "/api/v1/market/prices/{id}/history",
    tag = "market",
    params(("id" = Uuid, Path, description = "The instrument"), HistoryQuery),
    responses(
        (status = 200, description = "The observations inside the window", body = Vec<Price>),
        (status = 400, description = "The window ends before it starts", body = austeris_common::error::ErrorBody),
    ),
)]
async fn price_history(State(state): State<ServiceState>, Path(id): Path<Uuid>, Query(query): Query<HistoryQuery>) -> AppResult<Json<Vec<Price>>> {
    let to = query.to.unwrap_or_else(Utc::now);
    let from = query.from.unwrap_or(to - Duration::days(30));

    if from > to {
        return Err(AppError::bad_request(anyhow::anyhow!("the window ends before it starts")));
    }

    Ok(Json(repository::price_history(&state.pool, id, &query.currency, from, to).await?))
}

/// How much of a source's catalogue to take.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
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
#[derive(Debug, Serialize, utoipa::ToSchema)]
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
#[utoipa::path(
    post,
    path = "/api/v1/market/instruments/sync",
    tag = "market",
    params(SyncQuery),
    responses(
        (status = 200, description = "What was imported", body = Synced),
        (status = 503, description = "The source is switched off; set AUSTERIS_CMC_API_KEY", body = austeris_common::error::ErrorBody),
    ),
)]
async fn sync_instruments(State(state): State<ServiceState>, Query(query): Query<SyncQuery>) -> AppResult<Json<Synced>> {
    let source = state.sources.coinmarketcap();
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

/// Which day a refresh is for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct RefreshQuery {
    /// A past day, to fill in the official rates of a day this installation
    /// missed. The latest of everything when omitted.
    pub on: Option<NaiveDate>,
}

/// Asks every available source, records what it said, and hands official rates
/// on to the ledger.
///
/// The service does this by itself every hour; this is for not waiting, and
/// for a past day - a central bank answers for any date, so an exchange
/// recorded for last month can be measured against that day's rate. Sources are
/// independent: one failing costs its own prices and nothing else, and is named
/// in `failed`.
#[utoipa::path(
    post,
    path = "/api/v1/market/prices/refresh",
    tag = "market",
    params(RefreshQuery),
    responses((status = 200, description = "What was recorded, and which sources failed", body = Refreshed)),
)]
async fn refresh_prices(State(state): State<ServiceState>, Query(query): Query<RefreshQuery>) -> AppResult<Json<Refreshed>> {
    if query.on.is_some_and(|day| day > Utc::now().date_naive()) {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "no source publishes a rate for a day that has not happened"
        )));
    }
    Ok(Json(refresh::run(&state.pool, &state.sources, state.ledger.as_ref(), query.on).await?))
}

/// Every source, whether it is on, and how old what it last said is.
///
/// The answer to "why is this price from last Tuesday": a source switched off
/// for want of a key, or one that has stopped answering, shows here with the
/// time of its last observation.
#[utoipa::path(
    get,
    path = "/api/v1/market/sources",
    tag = "market",
    responses((status = 200, description = "Each source's state", body = Vec<SourceStatus>)),
)]
async fn list_sources(State(state): State<ServiceState>) -> AppResult<Json<Vec<SourceStatus>>> {
    Ok(Json(state.sources.status(&state.pool).await?))
}

/// This service's share of the platform's `OpenAPI` document.
///
/// The paths are the public ones - what a client calls through the gateway.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_instruments,
        read_instrument,
        create_instrument,
        bind_source,
        sync_instruments,
        latest_prices,
        price_history,
        refresh_prices,
        list_sources
    ),
    components(schemas(
        Instrument,
        Price,
        LatestPrice,
        Kind,
        NewInstrument,
        NewBinding,
        Synced,
        Refreshed,
        Failure,
        SourceStatus
    )),
    tags((name = "market", description = "Instruments and their prices")),
)]
pub struct ApiDoc;
