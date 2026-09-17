//! Who the gateway says is calling.
//!
//! Services do not check sessions; the gateway does, and passes on the user id
//! it established in a header. This is the extractor that reads it, so every
//! service reads it the same way and the name of the header is written once.
//!
//! The header is trustworthy only because the gateway strips any arriving from
//! outside before forwarding (see the gateway's `forward`). Services listen on
//! the private compose network and are never reachable directly - if that ever
//! stops being true, this is the one place the assumption is written down.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::AppError;

/// The header the gateway sets from a validated session.
pub const USER_HEADER: &str = "x-austeris-user-id";

/// The person this request is on behalf of.
///
/// A handler takes this and gets an id it can scope every query by; a request
/// without the header never reaches a handler at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller(pub Uuid);

impl Caller {
    /// The id, for scoping a query.
    #[must_use]
    pub fn id(self) -> Uuid {
        self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = AppError;

    // Nothing here awaits - reading a header is not I/O - but the trait's
    // method is async, so the work happens in `read` and this hands back a
    // future that is already finished.
    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Self::read(parts))
    }
}

impl Caller {
    /// Reads the caller out of a request's headers.
    fn read(parts: &Parts) -> Result<Self, AppError> {
        let header = parts
            .headers
            .get(USER_HEADER)
            .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, anyhow::anyhow!("not signed in")))?;

        // A header that is present but unreadable is a fault in the gateway,
        // not a client mistake: the client never sent it. Saying 500 here would
        // be honest but useless, and 401 is what the caller can act on - so it
        // is logged at the level that reaches an operator and answered as 401.
        let id = header.to_str().ok().and_then(|value| value.parse::<Uuid>().ok()).ok_or_else(|| {
            tracing::error!("the gateway set {USER_HEADER} to something that is not a user id");
            AppError::new(StatusCode::UNAUTHORIZED, anyhow::anyhow!("not signed in"))
        })?;

        Ok(Self(id))
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use uuid::Uuid;

    use super::{Caller, USER_HEADER};

    async fn extract(header: Option<&str>) -> Result<Caller, axum::http::StatusCode> {
        use axum::extract::FromRequestParts;

        let mut request = Request::builder();
        if let Some(value) = header {
            request = request.header(USER_HEADER, value);
        }
        let (mut parts, ()) = request.body(()).expect("building a request").into_parts();

        Caller::from_request_parts(&mut parts, &()).await.map_err(|error| error.status())
    }

    #[tokio::test]
    async fn the_header_the_gateway_sets_is_the_caller() {
        let id = Uuid::new_v4();
        assert_eq!(extract(Some(&id.to_string())).await.expect("extracting"), Caller(id));
    }

    #[tokio::test]
    async fn no_header_is_not_signed_in_rather_than_a_default_person() {
        // The tempting alternative - a nil uuid, or the first user - would make
        // an unauthenticated request read somebody's books.
        assert_eq!(extract(None).await.expect_err("should be refused"), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_header_that_is_not_an_id_is_refused_rather_than_parsed_loosely() {
        assert_eq!(
            extract(Some("not-a-uuid")).await.expect_err("should be refused"),
            axum::http::StatusCode::UNAUTHORIZED
        );
        assert_eq!(extract(Some("")).await.expect_err("should be refused"), axum::http::StatusCode::UNAUTHORIZED);
    }
}
