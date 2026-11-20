//! The request extractors every handler uses, with rejections that carry a code.
//!
//! axum's own [`Json`](axum::Json), [`Query`](axum::extract::Query) and
//! [`Path`](axum::extract::Path) answer a request they cannot read with a plain
//! text body. These wrap them and reject with an [`AppError`] instead, so a
//! malformed body, query or path answers with the same JSON as every other
//! error: `request.malformed`, and the reason axum gave in its `why` param.
//! A body that is not declared JSON is `request.not_json`.

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{AppError, Code};

/// A rejection of axum's turned into an error with a code: its status is kept,
/// and the reason it gave travels as `why`.
fn malformed(status: StatusCode, why: &str) -> AppError {
    AppError::refused(status, Code::RequestMalformed, anyhow::anyhow!("the request could not be read: {why}")).with_params(serde_json::json!({ "why": why }))
}

/// A JSON body, read into `T`; a response, written from `T`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(JsonRejection::MissingJsonContentType(rejection)) => Err(AppError::refused(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Code::RequestNotJson,
                anyhow::anyhow!("{}", rejection.body_text()),
            )),
            Err(rejection) => Err(malformed(rejection.status(), &rejection.body_text())),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// The query string, read into `T`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Self(value))
            .map_err(|rejection: QueryRejection| malformed(rejection.status(), &rejection.body_text()))
    }
}

/// The path's parameters, read into `T`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Self(value))
            .map_err(|rejection: PathRejection| malformed(rejection.status(), &rejection.body_text()))
    }
}
