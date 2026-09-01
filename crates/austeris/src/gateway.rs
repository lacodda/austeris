//! The gateway: the only surface austeris exposes to a network.
//!
//! It answers its own health probes, and forwards `/api/v1/{service}/...` to
//! the service that owns that prefix. Services listen on the private compose
//! network only, so the routing table here is also the access-control list:
//! a path with no entry cannot be reached at all.

use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::service::Service;

/// Builds the gateway's router.
pub fn router() -> Router {
    let mut api = Router::new();
    for service in Service::routed() {
        api = api.nest(&format!("/{service}"), forward(*service));
    }

    Router::new()
        .merge(austeris_common::health::routes(None))
        .nest("/api/v1", api)
        .fallback(not_found)
}

/// The subtree that carries one service's traffic.
///
/// Until a service exists to forward to, the prefix is reserved rather than
/// proxied: `Service::routed()` is empty, so this is never called yet. The
/// proxy lands with the first service that has a REST surface (v0.3.0).
fn forward(service: Service) -> Router {
    Router::new().fallback(move || async move { (StatusCode::NOT_IMPLEMENTED, format!("service `{service}` is not wired up yet\n")) })
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "no such endpoint\n")
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::router;

    async fn get(path: &str) -> (StatusCode, String) {
        let response = router().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn healthz_reports_ok() {
        let (status, body) = get("/healthz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn readyz_answers_without_a_database_of_its_own() {
        let (status, _) = get("/readyz").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn an_unrouted_path_is_not_reachable() {
        let (status, _) = get("/api/v1/ledger/accounts").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
