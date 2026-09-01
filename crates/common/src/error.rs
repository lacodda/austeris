//! One error type for every HTTP surface in the workspace.
//!
//! A handler returns [`AppResult`]; whatever goes wrong becomes an [`AppError`]
//! carrying a status, and axum renders it as the same JSON body everywhere. The
//! message reaching the client is deliberately the one the constructor was
//! given: no error the client cannot act on is spelled out to it.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// What a handler returns.
pub type AppResult<T> = Result<T, AppError>;

/// An error on its way to a client, with the status it should arrive as.
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    source: anyhow::Error,
}

impl AppError {
    /// Wraps an error with an explicit status.
    pub fn new(status: StatusCode, source: impl Into<anyhow::Error>) -> Self {
        Self { status, source: source.into() }
    }

    /// 500 — the service failed at something the client did not ask for wrongly.
    pub fn internal(source: impl Into<anyhow::Error>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, source)
    }

    /// 400 — the request itself is wrong.
    pub fn bad_request(source: impl Into<anyhow::Error>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, source)
    }

    /// 404 — the thing addressed does not exist.
    pub fn not_found(source: impl Into<anyhow::Error>) -> Self {
        Self::new(StatusCode::NOT_FOUND, source)
    }

    /// 503 — the service is up but something it depends on is not.
    pub fn unavailable(source: impl Into<anyhow::Error>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, source)
    }

    /// The status this will be rendered with.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.source()
    }
}

/// An unclassified error is a server fault: a handler that propagates one with
/// `?` gets a 500, not a misleading 400. A blanket impl over `Into<anyhow::Error>`
/// would collide with the standard `From<T> for T`, so the sources handlers
/// actually propagate are listed instead.
impl From<anyhow::Error> for AppError {
    fn from(source: anyhow::Error) -> Self {
        Self::internal(source)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(source: sqlx::Error) -> Self {
        Self::internal(source)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Server faults carry a stack of context worth keeping; client mistakes
        // are the client's to fix and only clutter the log at that level.
        if self.status.is_server_error() {
            tracing::error!(status = %self.status, error = ?self.source, "request failed");
        } else {
            tracing::debug!(status = %self.status, error = %self.source, "request rejected");
        }

        let body = ErrorBody {
            status: self.status.as_u16(),
            error: self.status.canonical_reason().unwrap_or("Error").to_owned(),
            message: self.source.to_string(),
        };
        (self.status, Json(body)).into_response()
    }
}

/// The JSON every failing endpoint in austeris answers with.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// The HTTP status, repeated in the body so a logged payload is self-contained.
    pub status: u16,
    /// The status' canonical name, e.g. `Not Found`.
    pub error: String,
    /// What went wrong, in the words the constructor was given.
    pub message: String,
}
