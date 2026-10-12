//! One error type for every HTTP surface in the workspace.
//!
//! A handler returns [`AppResult`]; whatever goes wrong becomes an [`AppError`]
//! carrying a status, and axum renders it as the same JSON body everywhere. The
//! message reaching the client is deliberately the one the constructor was
//! given: no error the client cannot act on is spelled out to it.
//!
//! An error a client is expected to act on - offer "record it again", say it
//! in the reader's language - also carries a [`Code`] and the facts its
//! message was written from. The message stays for the command line and the
//! logs; the code is what a screen decides by.

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
    code: Option<Code>,
    params: Option<serde_json::Value>,
}

impl AppError {
    /// Wraps an error with an explicit status.
    pub fn new(status: StatusCode, source: impl Into<anyhow::Error>) -> Self {
        Self {
            status,
            source: source.into(),
            code: None,
            params: None,
        }
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

    /// The same error, with the code a client decides by and the facts its
    /// message was written from - the names and numbers a translation of it
    /// needs.
    #[must_use]
    pub fn with_code(mut self, code: Code, params: serde_json::Value) -> Self {
        self.code = Some(code);
        self.params = Some(params);
        self
    }

    /// The status this will be rendered with.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The code this carries, when it carries one.
    #[must_use]
    pub fn code(&self) -> Option<Code> {
        self.code
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
            code: self.code.map(|code| code.as_str().to_owned()),
            params: self.params,
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
    /// What went wrong, as a stable identifier a client can act on and
    /// translate: `entry.possible_duplicate`. Absent on errors that carry
    /// none yet; `message` is always there to fall back on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The facts `message` was written from, by name, for a client writing
    /// it in another language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub params: Option<serde_json::Value>,
}

/// Declares the codes and the list of them in one place, so a code that is
/// answered but missing from [`Code::ALL`] cannot be written.
macro_rules! codes {
    ($($(#[$doc:meta])* $variant:ident = $text:literal,)*) => {
        /// What went wrong, as a client decides by it.
        ///
        /// Every code is in [`Code::ALL`], and a test holds the web interface
        /// to having a sentence for each in every language it speaks.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Code {
            $($(#[$doc])* $variant,)*
        }

        impl Code {
            /// Every code a service can answer with.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The code as it travels: `entry.possible_duplicate`.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }
        }
    };
}

codes! {
    /// An entry the same as this one was recorded a moment ago - a second
    /// press of the same button, most likely. Params: `entry_id`, `seconds`.
    PossibleDuplicate = "entry.possible_duplicate",
}

#[cfg(test)]
mod tests {
    use super::{AppError, Code};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[test]
    fn every_code_is_listed_once_and_reads_as_a_dotted_name() {
        let mut seen: Vec<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
        let listed = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), listed, "a code is listed twice");
        for code in seen {
            assert!(
                code.split('.').count() == 2 && code.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "`{code}` is not `area.what`"
            );
        }
    }

    #[tokio::test]
    async fn a_coded_error_carries_its_code_and_params_beside_the_message() {
        let error = AppError::bad_request(anyhow::anyhow!("recorded a moment ago")).with_code(Code::PossibleDuplicate, serde_json::json!({ "seconds": 3 }));
        let body = error.into_response().into_body().collect().await.expect("a body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["code"], "entry.possible_duplicate");
        assert_eq!(json["params"]["seconds"], 3);
        assert_eq!(json["message"], "recorded a moment ago");
    }

    #[tokio::test]
    async fn an_error_without_a_code_says_nothing_about_one() {
        let body = AppError::not_found(anyhow::anyhow!("no such entry"))
            .into_response()
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json.get("code").is_none(), "{json}");
        assert!(json.get("params").is_none(), "{json}");
    }
}
