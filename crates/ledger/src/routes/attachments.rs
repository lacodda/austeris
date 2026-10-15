//! Files kept with an entry: attaching one, reading it back, taking it off.

use austeris_common::{AppError, AppResult, Caller, Code};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::attachment::{self, MAX_BYTES};
use crate::model::Attachment;
use crate::repository::{self, Attached};

/// What attaching a file says beside its bytes.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AttachQuery {
    /// The file's name, as it should be saved again: `receipt.jpg`.
    pub name: String,
}

/// Keeps a file with an entry: a photo of the receipt, the bank's PDF, the
/// receipt's XML.
///
/// The body is the file itself, as it is. What it is is read from its bytes -
/// a JPEG, PNG, WebP or HEIC picture, a PDF, an XML document - and anything
/// else is refused whatever it is called or sent as. Ten megabytes at most.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/{id}/attachments",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry"), AttachQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The file, as it is"),
    responses(
        (status = 201, description = "The file, on the entry", body = Attachment),
        (status = 400, description = "The file has no name, or is empty", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such entry in the books", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The same file is on the entry already", body = austeris_common::error::ErrorBody),
        (status = 413, description = "The file is larger than ten megabytes", body = austeris_common::error::ErrorBody),
        (status = 415, description = "The file is not a picture, a PDF or an XML document", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn attach(
    State(pool): State<PgPool>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Query(query): Query<AttachQuery>,
    content: Bytes,
) -> AppResult<(StatusCode, Json<Attachment>)> {
    let name = attachment::file_name(&query.name).ok_or_else(|| AppError::bad_request(anyhow::anyhow!("a file needs a name")))?;
    if content.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("the file is empty")));
    }
    if content.len() > MAX_BYTES {
        return Err(AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            anyhow::anyhow!("a file can be {} MB at most", MAX_BYTES / (1024 * 1024)),
        )
        .with_code(Code::AttachmentTooLarge, serde_json::json!({ "limit_mb": MAX_BYTES / (1024 * 1024) })));
    }
    let media_type = attachment::media_type(&content).ok_or_else(|| {
        AppError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            anyhow::anyhow!("only a picture (JPEG, PNG, WebP, HEIC), a PDF or an XML document is kept"),
        )
        .with_code(Code::AttachmentUnsupported, serde_json::json!({}))
    })?;

    match repository::attach(&pool, caller.id(), &caller.into(), id, &name, media_type, &content).await? {
        Attached::Now(attachment) => Ok((StatusCode::CREATED, Json(attachment))),
        Attached::NoEntry => Err(AppError::not_found(anyhow::anyhow!("no such entry"))),
        Attached::Already(twin) => Err(
            AppError::new(StatusCode::CONFLICT, anyhow::anyhow!("this file is on the entry already, as {}", twin.name))
                .with_code(Code::AttachmentAlready, serde_json::json!({ "name": twin.name })),
        ),
    }
}

/// The file itself, under its own name.
///
/// A picture or a PDF opens in place; an XML document is only ever saved - a
/// document of that kind can be a page, and a page from this address would
/// run with the session that fetched it.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/attachments/{id}/content",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The file")),
    responses(
        (status = 200, description = "The file", content_type = "application/octet-stream", body = Vec<u8>),
        (status = 404, description = "No such file", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn content(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Response> {
    let (file, bytes) = repository::attachment_content(&pool, caller.id(), id)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such file")))?;

    let header = |value: String| HeaderValue::from_str(&value).map_err(AppError::internal);
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, header(file.media_type.clone())?);
    headers.insert(header::CONTENT_DISPOSITION, header(attachment::disposition(&file.media_type, &file.name))?);
    // What the type says it is, and nothing a browser guesses instead.
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !attachment::opens_in_place(&file.media_type) {
        headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    // A file's bytes never change under its id, so a phone opening the same
    // receipt twice does not fetch it twice - and only this browser keeps it.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=604800, immutable"));
    Ok(response)
}

/// Takes a file off its entry, to the trash: it can be put back for thirty
/// days.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/attachments/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The file")),
    responses(
        (status = 204, description = "In the trash"),
        (status = 404, description = "No such file on an entry in the books", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn remove(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::remove_attachment(&pool, caller.id(), &caller.into(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such file on an entry")))
    }
}
