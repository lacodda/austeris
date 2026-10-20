//! The imports service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards `/api/v1/imports/...`
//! here (ADR 0001), and every handler is scoped by the [`Caller`] it vouched
//! for.

use austeris_common::{AppError, AppResult, Caller, Code, health};
use austeris_ledger::attachment::{self, BODY_LIMIT, MAX_BYTES};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::MIGRATOR;
use crate::book::Said;
use crate::ledger::Books;
use crate::model::{Document, Needed, Origin, Reason, State as DocumentState};
use crate::receive::{self, Received};
use crate::repository::{self, Filter};
use crate::sifen::{Issuer, Item, Kind, Means, Payment, Receipt};
use crate::slip::{self, Ocr, OcrError, Reading};

/// What every handler is handed: this service's pool, and the books it posts
/// into.
#[derive(Clone)]
pub struct Imports<B> {
    pool: PgPool,
    books: B,
}

/// Builds the service's router.
pub fn router<B: Books + Clone + 'static>(pool: PgPool, books: B) -> Router {
    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool.clone(), &MIGRATOR))))
        .route(
            "/imports/documents",
            get(list_documents::<B>).post(upload::<B>).layer(DefaultBodyLimit::max(BODY_LIMIT)),
        )
        .route("/imports/documents/{id}", get(read_document::<B>).delete(dismiss::<B>))
        .route("/imports/documents/{id}/book", post(book::<B>))
        .route("/imports/slips", post(read_slip::<B>).layer(DefaultBodyLimit::max(BODY_LIMIT)))
        .with_state(Imports { pool, books })
}

/// What uploading a document says beside its bytes.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct UploadQuery {
    /// The file's name, as it should be kept: `factura.xml`.
    pub name: String,
    /// The account it was paid from, when the receipt does not say.
    pub account: Option<Uuid>,
    /// What it was for, when the shop has no usual category.
    pub category: Option<Uuid>,
}

/// What handing a document over came to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Upload {
    /// The document, as it now stands: booked, waiting, or refused.
    pub document: Document,
    /// It had arrived before - this file, or this receipt as another file.
    pub already: bool,
}

/// Hands a document over to be read into the books.
///
/// The body is the file itself. A SIFEN receipt's XML is read: one entry, a
/// line per item, the shop as its counterparty, the city as its place and the
/// file kept with it. What the receipt cannot say - which account, when it was
/// not paid in cash; what for, when the shop has no usual category - is taken
/// from `account` and `category`, or the document waits for it.
///
/// The same file, or the same receipt as another file, is answered with the
/// document it already is - booked now, if it waited for what was said here.
#[utoipa::path(
    post,
    path = "/api/v1/imports/documents",
    tag = "imports",
    params(UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The file, as it is"),
    responses(
        (status = 201, description = "Read, and booked, waiting or refused", body = Upload),
        (status = 200, description = "It had arrived before; as it now stands", body = Upload),
        (status = 400, description = "The file has no name, or is empty", body = austeris_common::error::ErrorBody),
        (status = 413, description = "The file is larger than ten megabytes", body = austeris_common::error::ErrorBody),
    ),
)]
async fn upload<B: Books + Clone + 'static>(
    State(state): State<Imports<B>>,
    caller: Caller,
    Query(query): Query<UploadQuery>,
    content: Bytes,
) -> AppResult<(StatusCode, Json<Upload>)> {
    let name = attachment::file_name(&query.name).ok_or_else(|| AppError::bad_request(anyhow::anyhow!("a file needs a name")))?;
    if content.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("the file is empty")));
    }
    if content.len() > MAX_BYTES {
        return Err(too_large(Code::DocumentTooLarge));
    }
    let said = Said {
        account: query.account,
        account_name: None,
        category: query.category,
    };
    let Received { document, already } = receive::receive(&state.pool, &state.books, caller.id(), &name, &content, Origin::Upload, said).await?;
    let status = if already { StatusCode::OK } else { StatusCode::CREATED };
    Ok((status, Json(Upload { document, already })))
}

/// Which documents to list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DocumentQuery {
    /// Only these: `waiting`, `booked` or `refused`.
    pub state: Option<DocumentState>,
    /// How many. Fifty when omitted, two hundred at most.
    pub limit: Option<i64>,
    /// How many to skip.
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/imports/documents",
    tag = "imports",
    params(DocumentQuery),
    responses((status = 200, description = "The documents, the newest first", body = Vec<Document>)),
)]
async fn list_documents<B: Books + Clone + 'static>(
    State(state): State<Imports<B>>,
    caller: Caller,
    Query(query): Query<DocumentQuery>,
) -> AppResult<Json<Vec<Document>>> {
    let filter = Filter {
        state: query.state,
        limit: query.limit.unwrap_or(50).clamp(1, 200),
        offset: query.offset.unwrap_or(0).max(0),
    };
    let documents = repository::documents(&state.pool, caller.id(), filter).await?;
    Ok(Json(documents.into_iter().map(repository::Stored::document).collect()))
}

#[utoipa::path(
    get,
    path = "/api/v1/imports/documents/{id}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "The document")),
    responses(
        (status = 200, description = "The document", body = Document),
        (status = 404, description = "No such document", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_document<B: Books + Clone + 'static>(State(state): State<Imports<B>>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Document>> {
    repository::document(&state.pool, caller.id(), id)
        .await?
        .map(|stored| Json(stored.document()))
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such document")))
}

/// What booking a waiting document carries: what it waits for.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BookBody {
    /// The account it was paid from.
    pub account_id: Option<Uuid>,
    /// What it was for. The shop takes it as its usual category when it has
    /// none, so the next receipt from there needs nothing said.
    pub category_id: Option<Uuid>,
}

/// Books a waiting document with what it waited for - or tries again, when it
/// waited for the books to answer.
///
/// What was said before stays for what is not said now. A document that is
/// booked or refused is answered as it is.
#[utoipa::path(
    post,
    path = "/api/v1/imports/documents/{id}/book",
    tag = "imports",
    params(("id" = Uuid, Path, description = "The document")),
    request_body = BookBody,
    responses(
        (status = 200, description = "The document: booked, or still waiting and why", body = Document),
        (status = 404, description = "No such document", body = austeris_common::error::ErrorBody),
    ),
)]
async fn book<B: Books + Clone + 'static>(
    State(state): State<Imports<B>>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<BookBody>,
) -> AppResult<Json<Document>> {
    let said = Said {
        account: body.account_id,
        account_name: None,
        category: body.category_id,
    };
    receive::book_again(&state.pool, &state.books, caller.id(), id, said)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such document")))
}

/// Dismisses a document: it leaves the list, with its file. An entry it
/// became stays in the books.
#[utoipa::path(
    delete,
    path = "/api/v1/imports/documents/{id}",
    tag = "imports",
    params(("id" = Uuid, Path, description = "The document")),
    responses(
        (status = 204, description = "Dismissed"),
        (status = 404, description = "No such document", body = austeris_common::error::ErrorBody),
    ),
)]
async fn dismiss<B: Books + Clone + 'static>(State(state): State<Imports<B>>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::dismiss(&state.pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such document")))
    }
}

/// Reads an exchange office's slip from a photo: the amounts that changed
/// hands, and the rate.
///
/// Read on this machine by Tesseract; the photo is not kept - the exchange it
/// fills in keeps it, once recorded. What is read is a suggestion for the
/// person to check, with the text it was read from.
#[utoipa::path(
    post,
    path = "/api/v1/imports/slips",
    tag = "imports",
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "The photo: JPEG, PNG or WebP"),
    responses(
        (status = 200, description = "What the slip says", body = Reading),
        (status = 413, description = "The photo is larger than ten megabytes", body = austeris_common::error::ErrorBody),
        (status = 415, description = "Not a JPEG, PNG or WebP picture", body = austeris_common::error::ErrorBody),
        (status = 503, description = "This installation has no OCR", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_slip<B: Books + Clone + 'static>(State(_state): State<Imports<B>>, _caller: Caller, content: Bytes) -> AppResult<Json<Reading>> {
    if content.len() > MAX_BYTES {
        return Err(too_large(Code::DocumentTooLarge));
    }
    match attachment::media_type(&content) {
        Some("image/jpeg" | "image/png" | "image/webp") => {}
        _ => {
            return Err(AppError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                anyhow::anyhow!("a slip is read from a JPEG, PNG or WebP photo"),
            )
            .with_code(Code::SlipUnsupported, serde_json::json!({})));
        }
    }
    let ocr = Ocr::find().ok_or_else(ocr_unavailable)?;
    let text = ocr.read(&content).await.map_err(|error| match error {
        OcrError::Unavailable => ocr_unavailable(),
        OcrError::Failed(_) => AppError::internal(error),
    })?;
    Ok(Json(slip::read_text(&text)))
}

/// Refused for size, the way a file kept with an entry is.
fn too_large(code: Code) -> AppError {
    let limit = MAX_BYTES / (1024 * 1024);
    AppError::new(StatusCode::PAYLOAD_TOO_LARGE, anyhow::anyhow!("a file can be {limit} MB at most")).with_code(code, serde_json::json!({ "limit_mb": limit }))
}

/// No OCR on this installation.
fn ocr_unavailable() -> AppError {
    AppError::unavailable(anyhow::anyhow!("this installation has no OCR; Tesseract is not installed"))
        .with_code(Code::SlipOcrUnavailable, serde_json::json!({}))
}

/// This service's share of the platform's `OpenAPI` document.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(upload, list_documents, read_document, book, dismiss, read_slip),
    components(schemas(
        Upload,
        Document,
        DocumentState,
        Origin,
        Needed,
        Reason,
        Receipt,
        Kind,
        Issuer,
        Item,
        Payment,
        Means,
        BookBody,
        Reading,
        slip::Money,
        slip::Rate,
    )),
    tags((name = "imports", description = "Documents read into the books: receipts uploaded or dropped in a folder, what each became or waits for, and exchange slips read from a photo")),
)]
pub struct ApiDoc;
