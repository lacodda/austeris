//! The portfolio service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards
//! `/api/v1/portfolio/...` here (ADR 0001), and every handler is scoped by the
//! [`Caller`] it vouched for.

use austeris_common::{AppError, AppResult, Caller, Code, health};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{Datelike, Days, NaiveDate, Utc};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::MIGRATOR;
use crate::gains;
use crate::model::{
    AccountBody, AccountValue, ClosedLot, Comparison, Difference, Gain, Gains, Holding, LineKind, Method, NewTradeBody, OpenLot, PortfolioAccount, Position,
    PositionLots, RevisionAction, Snapshot, SnapshotSummary, Trade, TradeBody, TradeKind, TradeRevision, TradeState, Valuation, Who, WhoKind,
};
use crate::repository::{self, TradeFilter};
use crate::services::{Books, Market};
use crate::snapshot;
use crate::trade::{self, Recorded, Refused};
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
        .route("/portfolio/accounts", get(list_accounts::<B, M>))
        .route("/portfolio/accounts/{id}", put(set_account::<B, M>))
        .route("/portfolio/lots", get(lots::<B, M>))
        .route("/portfolio/gains", get(gains::<B, M>))
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
///
/// Named by an `idempotency_key` a trade was recorded under before, it records
/// nothing and answers that trade as it is, with `200`: an import read twice,
/// or a request retried after its answer was lost, is one trade.
#[utoipa::path(
    post,
    path = "/api/v1/portfolio/trades",
    tag = "portfolio",
    request_body = NewTradeBody,
    responses(
        (status = 201, description = "The trade, its money booked", body = Trade),
        (status = 200, description = "A trade was recorded under that key before; this is it", body = Trade),
        (status = 400, description = "Not a trade this account, instrument or fee allows", body = austeris_common::error::ErrorBody),
        (status = 409, description = "It would sell or send more than was held at the time", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing was recorded", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_trade<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Json(body): Json<NewTradeBody>,
) -> AppResult<(StatusCode, Json<Trade>)> {
    let recorded = trade::record(
        &state.pool,
        &state.books,
        &state.market,
        caller.id(),
        &body.trade,
        body.idempotency_key.as_deref(),
    )
    .await
    .map_err(refused)?;
    Ok(match recorded {
        Recorded::New(trade) => (StatusCode::CREATED, Json(trade)),
        Recorded::Already(trade) => (StatusCode::OK, Json(trade)),
    })
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

/// Every broker's and wallet's account, and how each counts the lots its
/// sales take.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/accounts",
    tag = "portfolio",
    responses(
        (status = 200, description = "The accounts positions are held on, open ones first", body = Vec<PortfolioAccount>),
        (status = 503, description = "The ledger did not answer", body = austeris_common::error::ErrorBody),
    ),
)]
async fn list_accounts<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
) -> AppResult<Json<Vec<PortfolioAccount>>> {
    let methods = repository::cost_methods(&state.pool, caller.id()).await?;
    let accounts = state.books.accounts(caller.id()).await.map_err(|status| unvalued(Unvalued::Ledger(status)))?;
    Ok(Json(
        accounts
            .into_iter()
            .filter(crate::services::Account::holds_positions)
            .map(|account| PortfolioAccount {
                cost_method: methods.get(&account.id).copied().unwrap_or_default(),
                account_id: account.id,
                name: account.name,
                currency: account.currency,
                closed: account.closed,
            })
            .collect(),
    ))
}

/// Says how an account counts the lots its sales and transfers out take:
/// `fifo`, the oldest first; `lifo`, the newest first; or `average`, a share
/// of every lot. Every gain on the account is counted this way from then on,
/// the ones of years past as well - the lots are worked out from the trades.
#[utoipa::path(
    put,
    path = "/api/v1/portfolio/accounts/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The account")),
    request_body = AccountBody,
    responses(
        (status = 200, description = "The account, counted the way it now is", body = PortfolioAccount),
        (status = 400, description = "Positions are not held on that account", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such account", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger did not answer; nothing changed", body = austeris_common::error::ErrorBody),
    ),
)]
async fn set_account<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<AccountBody>,
) -> AppResult<Json<PortfolioAccount>> {
    let accounts = state.books.accounts(caller.id()).await.map_err(|status| unvalued(Unvalued::Ledger(status)))?;
    let account = accounts
        .into_iter()
        .find(|account| account.id == id)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account")))?;
    if !account.holds_positions() {
        return Err(refused(Refused::NotAHoldingAccount {
            name: account.name,
            kind: account.kind,
        }));
    }
    repository::set_cost_method(&state.pool, caller.id(), id, body.cost_method).await?;
    Ok(Json(PortfolioAccount {
        account_id: account.id,
        name: account.name,
        currency: account.currency,
        closed: account.closed,
        cost_method: body.cost_method,
    }))
}

/// Every position and the lots it is made of - each buy and receive still
/// held, or what is left of it after the sales that took from it - now, or
/// as of the end of a day. What each lot cost is in the currency it was
/// acquired in, the buy's fee included.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/lots",
    tag = "portfolio",
    params(OnQuery),
    responses(
        (status = 200, description = "Every position held, by account and instrument, with its lots, the oldest first", body = Vec<PositionLots>),
        (status = 400, description = "The day has not happened", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The market did not answer", body = austeris_common::error::ErrorBody),
    ),
)]
async fn lots<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Query(query): Query<OnQuery>,
) -> AppResult<Json<Vec<PositionLots>>> {
    let on = snapshot::day(query.on).map_err(|why| AppError::bad_request(anyhow::anyhow!(why)))?;
    let positions = gains::open_lots(&state.pool, &state.market, caller.id(), on).await.map_err(unvalued)?;
    Ok(Json(positions))
}

/// The period, and the currency, gains are asked for in.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct GainsQuery {
    /// The first day. The first of January of this year when omitted.
    pub from: Option<NaiveDate>,
    /// The last day. Today when omitted.
    pub to: Option<NaiveDate>,
    /// What to count them in. USD when omitted.
    pub currency: Option<String>,
}

/// What the sales of a period gained: each sale's proceeds, less its fee,
/// beside what the units it took cost - the lots its account's method picked
/// - and the sums. In one currency: proceeds at the rate of the sale's day,
/// each lot at the rate of the day it was acquired.
///
/// A sale needing a rate the books do not have is listed without its numbers,
/// left out of the sums, and its currency named in `unvalued`.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/gains",
    tag = "portfolio",
    params(GainsQuery),
    responses(
        (status = 200, description = "Every sale of the period, the latest first, and what they gained", body = Gains),
        (status = 400, description = "The currency is not a code, or the period ends before it starts", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer", body = austeris_common::error::ErrorBody),
    ),
)]
async fn gains<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Query(query): Query<GainsQuery>,
) -> AppResult<Json<Gains>> {
    let currency = currency(query.currency.as_deref())?;
    // Today somewhere east of UTC as well.
    let today = Utc::now().date_naive() + Days::new(1);
    let to = query.to.unwrap_or(today);
    let from = query.from.unwrap_or_else(|| NaiveDate::from_ymd_opt(to.year(), 1, 1).unwrap_or(to));
    if from > to {
        return Err(AppError::bad_request(anyhow::anyhow!("the period ends on {to}, before it starts on {from}")));
    }
    let report = gains::gains(&state.pool, &state.books, &state.market, caller.id(), from, to, &currency)
        .await
        .map_err(unvalued)?;
    Ok(Json(report))
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
    /// A name of the client's choosing, unique among the person's snapshots:
    /// taking one under a name already used answers the snapshot taken under
    /// it, as it was taken, rather than taking a second.
    #[schema(example = "austeris-2025:snapshot:4")]
    pub idempotency_key: Option<String>,
}

/// Takes a snapshot: the portfolio as it is now, or was on a day, kept as it
/// is - what it held, at what price, worth how much.
///
/// Named by an `idempotency_key` a snapshot was taken under before, it takes
/// nothing and answers that snapshot, with `200`.
#[utoipa::path(
    post,
    path = "/api/v1/portfolio/snapshots",
    tag = "portfolio",
    request_body = SnapshotBody,
    responses(
        (status = 201, description = "The snapshot", body = Snapshot),
        (status = 200, description = "A snapshot was taken under that key before; this is it", body = Snapshot),
        (status = 400, description = "The currency is not a code, the day has not happened, or the key is empty", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing was taken", body = austeris_common::error::ErrorBody),
    ),
)]
async fn take_snapshot<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Json(body): Json<SnapshotBody>,
) -> AppResult<(StatusCode, Json<Snapshot>)> {
    let key = trade::idempotency_key(body.idempotency_key.as_deref()).map_err(refused)?;
    if let Some(key) = key
        && let Some(taken) = repository::snapshot_by_key(&state.pool, caller.id(), key).await?
    {
        return Ok((StatusCode::OK, Json(taken)));
    }

    let currency = currency(body.currency.as_deref())?;
    let on = snapshot::day(body.on).map_err(|why| AppError::bad_request(anyhow::anyhow!(why)))?;
    let valuation = value::valuation(&state.pool, &state.books, &state.market, caller.id(), on, &currency)
        .await
        .map_err(unvalued)?;
    match repository::save_snapshot(&state.pool, caller.id(), &valuation, body.note.trim(), key).await {
        Ok(taken) => Ok((StatusCode::CREATED, Json(taken))),
        // Taken under the same key a moment ago, by a request racing this one.
        Err(error) if repository::key_taken(&error, repository::SNAPSHOT_KEY_TAKEN) => {
            let key = key.unwrap_or_default();
            let taken = repository::snapshot_by_key(&state.pool, caller.id(), key)
                .await?
                .ok_or_else(|| AppError::internal(anyhow::anyhow!("a snapshot's key was taken and then not found")))?;
            Ok((StatusCode::OK, Json(taken)))
        }
        Err(error) => Err(error.into()),
    }
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
        Refused::ReceiveUnpriced { symbol } => AppError::bad_request(message).with_code(Code::TradeReceiveUnpriced, serde_json::json!({ "symbol": symbol })),
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
        list_accounts,
        set_account,
        lots,
        gains,
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
        NewTradeBody,
        TradeRevision,
        TradeState,
        RevisionAction,
        Who,
        WhoKind,
        Position,
        PortfolioAccount,
        AccountBody,
        Method,
        PositionLots,
        OpenLot,
        Gains,
        Gain,
        ClosedLot,
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
    tags((name = "portfolio", description = "Trades on brokers' and exchanges' accounts, the money they move, what they add up to, the lots it is made of, what it is worth and gained, and snapshots of it")),
)]
pub struct ApiDoc;
