//! The portfolio service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards
//! `/api/v1/portfolio/...` here (ADR 0001), and every handler is scoped by the
//! [`Caller`] it vouched for.

use austeris_common::{AppError, AppResult, Caller, Code, health};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::MIGRATOR;
use crate::model::{
    AccountValue, Comparison, Difference, Holding, LineKind, Position, RevisionAction, Snapshot, SnapshotSummary, Trade, TradeBody, TradeKind, TradeRevision,
    TradeState, Valuation, Who, WhoKind,
};
use crate::repository::{self, TradeFilter};
use crate::services::{Books, Market};
use crate::snapshot;
use crate::trade::{self, Refused};
use crate::value::{self, Unvalued};

/// What every handler is handed: this service's pool, the ledger and the
/// market.
#[derive(Clone)]
pub struct Portfolio<B, M> {
    pool: PgPool,
    books: B,
    market: M,
}

/// Builds the service's router.
pub fn router<B: Books + Clone + 'static, M: Market + Clone + 'static>(pool: PgPool, books: B, market: M) -> Router {
    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool.clone(), &MIGRATOR))))
        .route("/portfolio/trades", get(list_trades::<B, M>).post(create_trade::<B, M>))
        .route(
            "/portfolio/trades/{id}",
            get(read_trade::<B, M>).put(replace_trade::<B, M>).delete(delete_trade::<B, M>),
        )
        .route("/portfolio/trades/{id}/history", get(trade_history::<B, M>))
        .route("/portfolio/positions", get(positions::<B, M>))
        .route("/portfolio/value", get(value::<B, M>))
        .route("/portfolio/snapshots", get(list_snapshots::<B, M>).post(take_snapshot::<B, M>))
        .route("/portfolio/snapshots/{id}", get(read_snapshot::<B, M>).delete(delete_snapshot::<B, M>))
        .with_state(Portfolio { pool, books, market })
}

/// Which trades to list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TradeQuery {
    /// Only those on this account, or arriving at it.
    pub account: Option<Uuid>,
    /// Only those of this instrument.
    pub instrument: Option<Uuid>,
    /// How many. A hundred when omitted, five hundred at most.
    pub limit: Option<i64>,
    /// How many to skip.
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/portfolio/trades",
    tag = "portfolio",
    params(TradeQuery),
    responses((status = 200, description = "The trades, the latest first", body = Vec<Trade>)),
)]
async fn list_trades<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Query(query): Query<TradeQuery>,
) -> AppResult<Json<Vec<Trade>>> {
    let filter = TradeFilter {
        account_id: query.account,
        instrument_id: query.instrument,
        limit: query.limit.unwrap_or(100).clamp(1, 500),
        offset: query.offset.unwrap_or(0).max(0),
    };
    let trades = repository::trades(&state.pool, caller.id(), &filter).await?;
    Ok(Json(trades.into_iter().map(trade::with_money).collect()))
}

/// Records a trade, and books its money.
///
/// A buy or a sale happens on a broker's or a wallet's account and moves its
/// money - in the account's currency, the price's and the fee's - as an entry
/// the portfolio keeps: the account's line, and the holding the money became
/// or came back from, which no report of spending counts. A transfer moves
/// units to another such account and no money; its fee is in units, as a
/// network takes it.
#[utoipa::path(
    post,
    path = "/api/v1/portfolio/trades",
    tag = "portfolio",
    request_body = TradeBody,
    responses(
        (status = 201, description = "The trade, its money booked", body = Trade),
        (status = 400, description = "Not a trade this account, instrument or fee allows", body = austeris_common::error::ErrorBody),
        (status = 409, description = "It would sell or send more than was held at the time", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing was recorded", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_trade<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Json(body): Json<TradeBody>,
) -> AppResult<(StatusCode, Json<Trade>)> {
    let trade = trade::record(&state.pool, &state.books, &state.market, caller.id(), &body)
        .await
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(trade)))
}

#[utoipa::path(
    get,
    path = "/api/v1/portfolio/trades/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    responses(
        (status = 200, description = "The trade", body = Trade),
        (status = 404, description = "No such trade", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_trade<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Trade>> {
    repository::trade(&state.pool, caller.id(), id)
        .await?
        .map(|found| Json(trade::with_money(found)))
        .ok_or_else(|| refused(Refused::Missing))
}

/// Says what a trade is now - every field, as recording it does - and
/// restates its money in the same entry.
#[utoipa::path(
    put,
    path = "/api/v1/portfolio/trades/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    request_body = TradeBody,
    responses(
        (status = 200, description = "The trade as it now is", body = Trade),
        (status = 400, description = "Not a trade this account, instrument or fee allows", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such trade", body = austeris_common::error::ErrorBody),
        (status = 409, description = "It would leave a position below nothing at some moment", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing changed", body = austeris_common::error::ErrorBody),
    ),
)]
async fn replace_trade<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<TradeBody>,
) -> AppResult<Json<Trade>> {
    let trade = trade::replace(&state.pool, &state.books, &state.market, caller.id(), id, &body)
        .await
        .map_err(refused)?;
    Ok(Json(trade))
}

/// Deletes a trade, and takes its money back out of the books. Its history
/// stays readable.
#[utoipa::path(
    delete,
    path = "/api/v1/portfolio/trades/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    responses(
        (status = 204, description = "Deleted, and its money with it"),
        (status = 404, description = "No such trade", body = austeris_common::error::ErrorBody),
        (status = 409, description = "A later sale or transfer would be left with nothing to sell", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger did not answer; nothing changed", body = austeris_common::error::ErrorBody),
    ),
)]
async fn delete_trade<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    trade::delete(&state.pool, &state.books, &state.market, caller.id(), id)
        .await
        .map_err(refused)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Everything that happened to a trade, oldest first: who recorded, changed
/// or deleted it, and what it was before and after.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/trades/{id}/history",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    responses(
        (status = 200, description = "Its history, oldest first", body = Vec<TradeRevision>),
        (status = 404, description = "No trade by that id, ever", body = austeris_common::error::ErrorBody),
    ),
)]
async fn trade_history<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<TradeRevision>>> {
    repository::history(&state.pool, caller.id(), id)
        .await?
        .map(Json)
        .ok_or_else(|| refused(Refused::Missing))
}

/// The day something is asked about.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct OnQuery {
    /// The day: the trades of it and before. Every trade when omitted.
    pub on: Option<NaiveDate>,
}

/// What the trades add up to: units of each instrument on each account - all
/// of them, or as of a day. Positions with nothing left are left out.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/positions",
    tag = "portfolio",
    params(OnQuery),
    responses((status = 200, description = "Every position not empty, by account and instrument", body = Vec<Position>)),
)]
async fn positions<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Query(query): Query<OnQuery>,
) -> AppResult<Json<Vec<Position>>> {
    let positions = repository::positions(&state.pool, caller.id(), query.on).await?;
    Ok(Json(
        positions
            .into_iter()
            .map(|position| Position {
                quantity: position.quantity.normalize(),
                ..position
            })
            .collect(),
    ))
}

/// What a valuation is asked in, and of when.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ValueQuery {
    /// The currency to value it in. USD when omitted.
    pub currency: Option<String>,
    /// A day past: its trades and the money at its end, at the prices of
    /// its end. Now, at the latest prices, when omitted.
    pub on: Option<NaiveDate>,
}

/// What every broker's and wallet's account holds and what it is worth: the
/// money on it at the ledger's rate - a stablecoin is money there - and each
/// position at the market's price.
///
/// What has no price or rate in the currency asked for is listed with its
/// quantity and named in `unvalued`, and is not in the total.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/value",
    tag = "portfolio",
    params(ValueQuery),
    responses(
        (status = 200, description = "Each account, what is on it, what it is worth, and the total", body = Valuation),
        (status = 400, description = "The currency is not a code, or the day has not happened", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer", body = austeris_common::error::ErrorBody),
    ),
)]
async fn value<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Query(query): Query<ValueQuery>,
) -> AppResult<Json<Valuation>> {
    let currency = currency(query.currency.as_deref())?;
    let on = snapshot::day(query.on).map_err(|why| AppError::bad_request(anyhow::anyhow!(why)))?;
    let valuation = value::valuation(&state.pool, &state.books, &state.market, caller.id(), on, &currency)
        .await
        .map_err(unvalued)?;
    Ok(Json(valuation))
}

#[utoipa::path(
    get,
    path = "/api/v1/portfolio/snapshots",
    tag = "portfolio",
    responses((status = 200, description = "The snapshots, the latest moment first", body = Vec<SnapshotSummary>)),
)]
async fn list_snapshots<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
) -> AppResult<Json<Vec<SnapshotSummary>>> {
    Ok(Json(repository::snapshots(&state.pool, caller.id()).await?))
}

/// What taking a snapshot carries.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct SnapshotBody {
    /// The day it shows: a day past is its trades and the money at its end,
    /// at the prices of its end. Now when omitted.
    pub on: Option<NaiveDate>,
    /// What to value it in. USD when omitted.
    pub currency: Option<String>,
    /// What to write on it.
    #[serde(default)]
    pub note: String,
}

/// Takes a snapshot: the portfolio as it is now, or was on a day, kept as it
/// is - what it held, at what price, worth how much.
#[utoipa::path(
    post,
    path = "/api/v1/portfolio/snapshots",
    tag = "portfolio",
    request_body = SnapshotBody,
    responses(
        (status = 201, description = "The snapshot", body = Snapshot),
        (status = 400, description = "The currency is not a code, or the day has not happened", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing was taken", body = austeris_common::error::ErrorBody),
    ),
)]
async fn take_snapshot<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Json(body): Json<SnapshotBody>,
) -> AppResult<(StatusCode, Json<Snapshot>)> {
    let currency = currency(body.currency.as_deref())?;
    let on = snapshot::day(body.on).map_err(|why| AppError::bad_request(anyhow::anyhow!(why)))?;
    let valuation = value::valuation(&state.pool, &state.books, &state.market, caller.id(), on, &currency)
        .await
        .map_err(unvalued)?;
    let taken = repository::save_snapshot(&state.pool, caller.id(), &valuation, body.note.trim()).await?;
    Ok((StatusCode::CREATED, Json(taken)))
}

/// What a snapshot is set beside.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct CompareQuery {
    /// Another snapshot, to see what moved between the two. Now when omitted.
    pub against: Option<Uuid>,
}

/// A snapshot as it was taken, beside the portfolio now - or beside another
/// snapshot - in the snapshot's currency, and what moved between them.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/snapshots/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The snapshot"), CompareQuery),
    responses(
        (status = 200, description = "The snapshot, what it is compared with, and every line of either", body = Comparison),
        (status = 404, description = "No such snapshot", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_snapshot<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<CompareQuery>,
) -> AppResult<Json<Comparison>> {
    let no_snapshot = || AppError::not_found(anyhow::anyhow!("no such snapshot"));
    let taken = repository::snapshot(&state.pool, caller.id(), id).await?.ok_or_else(no_snapshot)?;
    let against = match query.against {
        Some(other) => repository::snapshot(&state.pool, caller.id(), other).await?.ok_or_else(no_snapshot)?.valuation,
        None => value::valuation(&state.pool, &state.books, &state.market, caller.id(), None, &taken.valuation.currency)
            .await
            .map_err(unvalued)?,
    };
    Ok(Json(Comparison {
        differences: snapshot::differences(&taken.valuation, &against),
        snapshot: taken,
        against,
        against_id: query.against,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/portfolio/snapshots/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The snapshot")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such snapshot", body = austeris_common::error::ErrorBody),
    ),
)]
async fn delete_snapshot<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if repository::delete_snapshot(&state.pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such snapshot")))
    }
}

/// A currency a valuation is asked in: USD when none is.
fn currency(asked: Option<&str>) -> AppResult<String> {
    match asked {
        None => Ok("USD".to_owned()),
        Some(raw) => austeris_ledger::currency::code(raw).ok_or_else(|| {
            AppError::bad_request(anyhow::anyhow!(
                "`{raw}` is not a currency code; three letters, as in USD, or an exchange's ticker, as in USDT"
            ))
        }),
    }
}

/// A refused trade, as the caller is told.
fn refused(refusal: Refused) -> AppError {
    let message = anyhow::anyhow!("{refusal}");
    match refusal {
        Refused::Missing => AppError::not_found(message),
        Refused::Malformed(_) => AppError::bad_request(message),
        Refused::UnknownAccount => AppError::bad_request(message).with_code(Code::TradeUnknownAccount, serde_json::json!({})),
        Refused::UnknownInstrument => AppError::bad_request(message).with_code(Code::TradeUnknownInstrument, serde_json::json!({})),
        Refused::NotAHoldingAccount { name, kind } => {
            AppError::bad_request(message).with_code(Code::TradeNotAHoldingAccount, serde_json::json!({ "name": name, "kind": kind }))
        }
        Refused::ClosedAccount { name } => AppError::bad_request(message).with_code(Code::TradeClosedAccount, serde_json::json!({ "name": name })),
        Refused::MoneyNotHolding { symbol } => AppError::bad_request(message).with_code(Code::TradeMoneyNotHolding, serde_json::json!({ "symbol": symbol })),
        Refused::FeeTooLarge => AppError::bad_request(message).with_code(Code::TradeFeeTooLarge, serde_json::json!({})),
        Refused::NothingMoved { currency } => AppError::bad_request(message).with_code(Code::TradeNothingMoved, serde_json::json!({ "currency": currency })),
        Refused::Oversold { symbol, short, at } => {
            AppError::new(StatusCode::CONFLICT, message).with_code(Code::TradeOversold, serde_json::json!({ "symbol": symbol, "short": short, "at": at }))
        }
        Refused::RefusedByLedger(said) => AppError::bad_request(message).with_code(Code::TradeRefusedByLedger, serde_json::json!({ "message": said })),
        Refused::LedgerUnavailable => AppError::unavailable(message).with_code(Code::PortfolioLedgerUnavailable, serde_json::json!({})),
        Refused::MarketUnavailable => AppError::unavailable(message).with_code(Code::PortfolioMarketUnavailable, serde_json::json!({})),
        Refused::Failed(error) => AppError::internal(error),
    }
}

/// A valuation that could not be made, as the caller is told.
fn unvalued(error: Unvalued) -> AppError {
    match error {
        Unvalued::Ledger(status) => {
            tracing::error!(%status, "the ledger did not answer a valuation");
            AppError::unavailable(anyhow::anyhow!("the ledger is not answering")).with_code(Code::PortfolioLedgerUnavailable, serde_json::json!({}))
        }
        Unvalued::Market(status) => {
            tracing::error!(%status, "the market did not answer a valuation");
            AppError::unavailable(anyhow::anyhow!("the market is not answering")).with_code(Code::PortfolioMarketUnavailable, serde_json::json!({}))
        }
        Unvalued::Here(error) => AppError::internal(error),
    }
}

/// This service's share of the platform's `OpenAPI` document.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_trades,
        create_trade,
        read_trade,
        replace_trade,
        delete_trade,
        trade_history,
        positions,
        value,
        list_snapshots,
        take_snapshot,
        read_snapshot,
        delete_snapshot
    ),
    components(schemas(
        Trade,
        TradeKind,
        TradeBody,
        TradeRevision,
        TradeState,
        RevisionAction,
        Who,
        WhoKind,
        Position,
        Valuation,
        AccountValue,
        Holding,
        LineKind,
        Snapshot,
        SnapshotSummary,
        SnapshotBody,
        Comparison,
        Difference,
    )),
    tags((name = "portfolio", description = "Trades on brokers' and exchanges' accounts, the money they move, what they add up to, what it is worth, and snapshots of it")),
)]
pub struct ApiDoc;
