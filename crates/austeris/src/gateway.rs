//! The gateway: the only surface austeris exposes to a network.
//!
//! It answers its own health probes and forwards `/api/v1/{prefix}/...` to the
//! service that owns that prefix. Services listen on the private compose
//! network only, so the routing table here is also the access-control list: a
//! path with no entry cannot be reached at all.

use austeris_common::{AppError, AppResult, health};
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::service::Service;

/// The header a service reads to learn who is calling.
///
/// Set by the gateway from a validated session and stripped from anything
/// arriving from outside - otherwise anyone could simply send it.
pub const USER_HEADER: &str = "x-austeris-user-id";

/// What forwarding needs: one client, reused across requests.
///
/// A client per request would open a new connection every time and leak file
/// descriptors under load; reqwest's client is a pool and is meant to be shared.
#[derive(Clone)]
struct Upstream {
    client: reqwest::Client,
    service: Service,
}

/// Builds the gateway's router.
pub fn router() -> Router {
    let client = reqwest::Client::new();

    let mut api = Router::new();
    for service in Service::routed() {
        let upstream = Upstream {
            client: client.clone(),
            service: *service,
        };
        // Wildcard routes rather than a nested fallback: a `fallback` inside a
        // `nest` is only reached when the outer router has no answer, and the
        // outer one always does - its own 404.
        //
        // Each service's routes carry their own upstream, so `with_state` is
        // applied per subtree and every one of them is a plain `Router` by the
        // time it is merged.
        let routes: Router = Router::new()
            .route("/", axum::routing::any(forward))
            .route("/{*rest}", axum::routing::any(forward))
            .with_state(upstream);
        api = api.nest(&format!("/{}", service.prefix()), routes);
    }

    Router::new()
        .merge(health::routes::<()>(None))
        .nest("/api/v1", api)
        .fallback(not_found)
        // Applied last so it wraps everything, health probes included: an
        // orchestrator polls at a fixed low rate, and a probe that could bypass
        // the limit would be a way around it.
        .layer(axum::middleware::from_fn_with_state(
            crate::ratelimit::Limiter::default(),
            crate::ratelimit::limit,
        ))
}

/// Passes a request to the service that owns its prefix, and its answer back.
async fn forward(State(upstream): State<Upstream>, request: Request) -> AppResult<Response> {
    let (parts, body) = request.into_parts();

    // `nest` strips the prefix, so what arrives here is the path as the service
    // knows it. The service's own routes carry the prefix back (`/auth/login`),
    // which keeps a service's paths readable in its own crate.
    let path = parts.uri.path_and_query().map_or("/", |p| p.as_str());
    let url = format!("{}/{}{path}", upstream.service.address(), upstream.service.prefix());
    let url: reqwest::Url = url
        .parse()
        .map_err(|error| AppError::internal(anyhow::anyhow!("{} has an unusable address: {error}", upstream.service)))?;

    let body = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|_| AppError::new(StatusCode::PAYLOAD_TOO_LARGE, anyhow::anyhow!("the request body is too large")))?;

    let mut outgoing = upstream.client.request(parts.method.clone(), url).body(body);
    for (name, value) in &parts.headers {
        // Hop-by-hop headers describe this connection, not the request; passing
        // them on makes the upstream answer for a connection it is not part of.
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        // The identity header is the gateway's word, not the caller's. One
        // arriving from outside is dropped here, before anything downstream
        // could believe it.
        if name.as_str().eq_ignore_ascii_case(USER_HEADER) {
            continue;
        }
        outgoing = outgoing.header(name, value);
    }

    // identity is asked about its own sessions over REST, so it is not asked
    // about them over gRPC first - that would be a second round trip to learn
    // what the call itself is about to establish, and signing in would need a
    // session to sign in with.
    if upstream.service != Service::Identity {
        // A session is required, not merely passed on when present. Everything
        // behind this gateway is one person's finances; an installation on a
        // home network must not serve them to whoever asks.
        let Some(user_id) = caller(&parts).await else {
            return Err(AppError::new(StatusCode::UNAUTHORIZED, anyhow::anyhow!("not signed in")));
        };
        outgoing = outgoing.header(USER_HEADER, user_id);
    }

    let outgoing = outgoing.build().map_err(AppError::internal)?;
    let response = upstream.client.execute(outgoing).await.map_err(|error| {
        // A service that is down is this deployment's problem, not the
        // caller's mistake - and the caller can usefully retry.
        tracing::error!(service = upstream.service.as_str(), %error, "forwarding failed");
        AppError::new(StatusCode::BAD_GATEWAY, anyhow::anyhow!("{} is not answering", upstream.service))
    })?;

    let mut builder = Response::builder().status(response.status());
    for (name, value) in response.headers() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }

    let bytes = response.bytes().await.map_err(AppError::internal)?;
    builder.body(Body::from(bytes)).map_err(AppError::internal)
}

/// Asks identity who the session cookie belongs to, if there is one.
///
/// `None` covers every way of not knowing: no cookie, an invalid session, or an
/// identity service that cannot be reached. The caller is refused in all three
/// - an installation that cannot check who is asking must not answer.
async fn caller(parts: &axum::http::request::Parts) -> Option<String> {
    let token = session_cookie(parts)?;

    let mut client = austeris_proto::identity::v1::identity_client::IdentityClient::connect(Service::Identity.grpc_address())
        .await
        .inspect_err(|error| tracing::error!(%error, "could not reach identity to validate a session"))
        .ok()?;

    let response = client
        .validate_session(austeris_proto::identity::v1::ValidateSessionRequest { token })
        .await
        .inspect_err(|error| tracing::error!(%error, "validating a session failed"))
        .ok()?
        .into_inner();

    // Empty means unknown, expired or deactivated - three cases nobody
    // downstream has any business telling apart.
    (!response.user_id.is_empty()).then_some(response.user_id)
}

/// Pulls the session token out of the request's cookies.
fn session_cookie(parts: &axum::http::request::Parts) -> Option<String> {
    let header = parts.headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == austeris_identity::session::COOKIE).then(|| value.trim().to_owned())
    })
}

/// The largest request the gateway will carry.
///
/// Generous for JSON, and a bound rather than none: an unbounded body is a way
/// to exhaust a Raspberry Pi's memory from outside.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Whether a header describes the connection rather than the request.
///
/// Listed per RFC 9110 §7.6.1. `Host` is separate: reqwest sets it for the
/// upstream, and copying the client's would send the service the gateway's
/// public name.
fn is_hop_by_hop(name: &str) -> bool {
    const HOP_BY_HOP: &[&str] = &[
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        // Set by reqwest from the body it is actually sending; a copied one can
        // disagree with it and truncate the request.
        "content-length",
    ];
    HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str())
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "no such endpoint\n")
}

/// Whether a URI names a path the gateway will forward.
///
/// Used by the tests below and nowhere else: the routing itself is `nest`'s job.
#[cfg(test)]
fn is_routed(uri: &axum::http::Uri) -> bool {
    Service::routed()
        .iter()
        .any(|service| uri.path().starts_with(&format!("/api/v1/{}/", service.prefix())))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode, Uri};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{is_hop_by_hop, is_routed, router};

    async fn get(path: &str) -> (StatusCode, String) {
        // The rate limiter reads the peer address, which `oneshot` does not
        // supply on its own - the real server does it through
        // `into_make_service_with_connect_info`.
        let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000))));

        let response = router().oneshot(request).await.unwrap();
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

    #[tokio::test]
    async fn a_service_behind_the_gateway_is_not_served_without_a_session() {
        // Everything behind here is one person's finances. Before this check
        // existed, `market` answered anyone on the network - found by running
        // it, not by a test.
        let (status, _) = get("/api/v1/market/instruments").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn signing_in_does_not_itself_need_a_session() {
        // identity is exempt, or there would be no way to obtain the session
        // every other path demands. It is not running here, so 502 is the
        // proof the request was forwarded rather than refused.
        let (status, _) = get("/api/v1/auth/login").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn a_routed_path_reaches_the_proxy_rather_than_the_fallback() {
        // identity is not running here, so the answer is 502 - which is the
        // point: a 404 would mean the request never left the gateway.
        let (status, _) = get("/api/v1/auth/me").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn routing_covers_every_service_prefix_and_nothing_else() {
        assert!(is_routed(&Uri::from_static("/api/v1/auth/login")));
        assert!(!is_routed(&Uri::from_static("/api/v1/ledger/accounts")));
        // The prefix must be a whole segment: a service called `auth` must not
        // capture `/api/v1/authority/...`.
        assert!(!is_routed(&Uri::from_static("/api/v1/authority/x")));
    }

    #[test]
    fn connection_headers_are_not_passed_on() {
        assert!(is_hop_by_hop("Connection"));
        assert!(is_hop_by_hop("transfer-encoding"));
        // Content-Length is set from the body actually being sent; a copied one
        // can disagree with it and truncate the request.
        assert!(is_hop_by_hop("Content-Length"));
        // Host would tell the service the gateway's public name.
        assert!(is_hop_by_hop("host"));

        assert!(!is_hop_by_hop("cookie"), "the session cookie must survive the hop");
        assert!(!is_hop_by_hop("content-type"));
        assert!(!is_hop_by_hop("authorization"));
    }
}
