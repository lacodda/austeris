//! The portfolio's trash, over HTTP: deleted trades, what brings one back,
//! and what erases one for good.

use austeris_common::extract::{Json, Path};
use austeris_common::{AppResult, Caller};
use axum::extract::State;
use axum::http::StatusCode;
use uuid::Uuid;

use super::{Portfolio, refused};
use crate::model::{Trade, TradeTrash};
use crate::services::{Books, Market};
use crate::trade::{self, Refused};
use crate::trash;

/// The deleted trades the trash still keeps, the most recently deleted first,
/// and how many days it keeps one.
#[utoipa::path(
    get,
    path = "/api/v1/portfolio/trash",
    tag = "portfolio",
    responses((status = 200, description = "What is in the trash", body = TradeTrash)),
)]
pub(super) async fn list<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
) -> AppResult<Json<TradeTrash>> {
    Ok(Json(trash::trash(&state.pool, caller.id()).await?))
}

/// Brings a deleted trade back: under its own id, as it was, with its money
/// booked again under the key it was booked under.
#[utoipa::path(
    post,
    path = "/api/v1/portfolio/trash/trades/{id}/restore",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    responses(
        (status = 200, description = "Back, with its money", body = Trade),
        (status = 400, description = "It no longer fits the books: its account is closed", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such trade in the trash", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Its units were sold since, or a trade recorded since under the same key stands for it", body = austeris_common::error::ErrorBody),
        (status = 503, description = "The ledger or the market did not answer; nothing changed", body = austeris_common::error::ErrorBody),
    ),
)]
pub(super) async fn restore<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Trade>> {
    let trade = trade::restore(&state.pool, &state.books, &state.market, caller.id(), id)
        .await
        .map_err(refused)?;
    Ok(Json(trade))
}

/// Erases a deleted trade for good, with its history.
#[utoipa::path(
    delete,
    path = "/api/v1/portfolio/trash/trades/{id}",
    tag = "portfolio",
    params(("id" = Uuid, Path, description = "The trade")),
    responses(
        (status = 204, description = "Erased"),
        (status = 404, description = "No such trade in the trash", body = austeris_common::error::ErrorBody),
    ),
)]
pub(super) async fn erase<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    if trash::erase(&state.pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(refused(Refused::NotInTrash))
    }
}

/// Erases everything in the trash.
#[utoipa::path(
    delete,
    path = "/api/v1/portfolio/trash",
    tag = "portfolio",
    responses((status = 204, description = "The trash is empty")),
)]
pub(super) async fn empty<B: Books + Clone + 'static, M: Market + Clone + 'static>(
    State(state): State<Portfolio<B, M>>,
    caller: Caller,
) -> AppResult<StatusCode> {
    trash::empty(&state.pool, caller.id()).await?;
    Ok(StatusCode::NO_CONTENT)
}
