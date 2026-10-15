//! What an entry remembers - who changed it and what it was - and the trash
//! that deleted entries and the files taken off them wait in.

use austeris_common::{AppError, AppResult, Caller, Code};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Attachment, Entry, Revision, Trash};
use crate::repository::{self, FileRestored, RestoreRefused, Restored};

/// Everything that happened to an entry, oldest first: who recorded it, each
/// change with what it was before and after, its deletion and its return.
///
/// For an entry in the trash as well: its history is how it is restored.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/entries/{id}/history",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    responses(
        (status = 200, description = "What happened to it, oldest first", body = Vec<Revision>),
        (status = 404, description = "No such entry, in the books or in the trash", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn history(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Vec<Revision>>> {
    repository::history(&pool, caller.id(), id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such entry")))
}

/// What is in the trash: entries deleted, and files taken off entries, for
/// thirty days after - then they are erased for good.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/trash",
    tag = "ledger",
    responses((status = 200, description = "The trash, the most recently deleted first", body = Trash)),
)]
pub async fn trash(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Trash>> {
    Ok(Json(repository::trash(&pool, caller.id()).await?))
}

/// Erases everything in the trash, for good.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/trash",
    tag = "ledger",
    responses((status = 204, description = "Emptied")),
)]
pub async fn empty(State(pool): State<PgPool>, caller: Caller) -> AppResult<StatusCode> {
    repository::empty_trash(&pool, caller.id()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Brings a deleted entry back, as it was: the same id and lines, its tags,
/// counterparty and place, and its files. A counterparty, place or tag deleted
/// while it was in the trash is found or made again by its name.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/trash/entries/{id}/restore",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The deleted entry")),
    responses(
        (status = 200, description = "The entry, back in the books", body = Entry),
        (status = 404, description = "No such entry in the trash", body = austeris_common::error::ErrorBody),
        (status = 409, description = "It moved money on an account or under a category that is gone, or its module posted it again since", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn restore_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Entry>> {
    match repository::restore_entry(&pool, caller.id(), &caller.into(), id).await {
        Ok(Restored::Back(entry)) => Ok(Json(*entry)),
        Ok(Restored::Missing) => Err(AppError::not_found(anyhow::anyhow!("no such entry in the trash"))),
        Err(error) => match error.downcast::<RestoreRefused>() {
            Ok(refused) => Err(AppError::new(StatusCode::CONFLICT, refused)),
            Err(error) => Err(AppError::internal(error)),
        },
    }
}

/// Erases a deleted entry for good, with its history and its files.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/trash/entries/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The deleted entry")),
    responses(
        (status = 204, description = "Erased"),
        (status = 404, description = "No such entry in the trash", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn erase_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::erase_entry(&pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such entry in the trash")))
    }
}

/// Puts a file taken off an entry back on it.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/trash/attachments/{id}/restore",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The file")),
    responses(
        (status = 200, description = "The file, on its entry again", body = Attachment),
        (status = 404, description = "No such file in the trash", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Its entry is in the trash too, or the same file is on the entry again", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn restore_attachment(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Attachment>> {
    match repository::restore_attachment(&pool, caller.id(), &caller.into(), id).await? {
        FileRestored::Back(attachment) => Ok(Json(attachment)),
        FileRestored::Missing => Err(AppError::not_found(anyhow::anyhow!("no such file in the trash"))),
        FileRestored::EntryInTrash => Err(AppError::new(
            StatusCode::CONFLICT,
            anyhow::anyhow!("the entry this file was on is in the trash; restore the entry and its files come back with it"),
        )),
        FileRestored::AttachedAgain => Err(AppError::new(StatusCode::CONFLICT, anyhow::anyhow!("the same file is on the entry again"))
            .with_code(Code::AttachmentAlready, serde_json::json!({}))),
    }
}

/// Erases a file taken off an entry, for good.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/trash/attachments/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The file")),
    responses(
        (status = 204, description = "Erased"),
        (status = 404, description = "No such file in the trash", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn erase_attachment(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::erase_attachment(&pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such file in the trash")))
    }
}
