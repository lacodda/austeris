//! Serving the web interface.
//!
//! The built SPA is compiled into the binary (ADR 0011): a self-hosted install
//! is one file, and the interface can never be from a different build than the
//! API it calls. In development the app is served by Vite, which proxies `/api`
//! to a running austeris, so this path is what a release does rather than what
//! a developer waits for.
//!
//! The gateway's fallback is the whole routing story. A single-page app owns
//! its own paths, so anything the API has not claimed is answered with
//! `index.html` and the browser decides what to draw - a refresh on `/entries`
//! would otherwise 404, since only the app knows the route. Everything under
//! `/api` is the exception: a path there that no service owns is a mistake in
//! a client, and answering it with a web page would hide that behind a 200.

use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

/// The contents of `web/dist`, as of compile time.
///
/// `allow_missing` because the directory is a build output: it is gitignored,
/// and `pnpm build` empties it before writing, so nothing committed can keep it
/// in place. Without this a clean checkout would not compile, and a Rust-only
/// contributor would be stopped by a missing Node toolchain. A binary built
/// that way says so on every page rather than serving a blank one, and
/// `release_consistency` refuses to cut a version without the build.
#[derive(Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

/// What the document may load, and who may frame it.
///
/// Everything comes from this origin: the bundles, the API, the mark. Inline
/// styles are allowed because the component kit sets layout through `style`
/// (a grid's tracks, a popup's position) and the overlays insert their own
/// rules at run time; inline scripts are not, and the build writes none.
/// `frame-ancestors 'none'` because a page holding someone's finances has no
/// business inside another site's frame, where a click can be stolen.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; \
     form-action 'self'; frame-ancestors 'none'";

/// Answers a request that no route matched: an asset if there is one at that
/// path, the application document otherwise - except under `/api`.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return crate::gateway::not_found();
    }

    let file = path.trim_start_matches('/');
    if file.is_empty() || file == "index.html" {
        return document();
    }

    match Assets::get(file) {
        Some(asset) => {
            // The type was decided when the file was embedded, for this exact
            // file, rather than guessed again here from a path.
            let mime = asset.metadata.mimetype().to_owned();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, cache_control(file).to_owned()),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
                ],
                asset.data,
            )
                .into_response()
        }
        // Not a file: a route of the app's own, and the app resolves it -
        // including the ones it will itself call unknown, which the server
        // cannot tell from the ones it has never heard of.
        None => document(),
    }
}

/// The application document.
fn document() -> Response {
    let Some(index) = Assets::get("index.html") else {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "no web interface was built into this binary; the API is at /api/v1 and its description at /docs\n",
        )
            .into_response();
    };

    let mut response = (StatusCode::OK, index.data).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    // Never cached: it names the fingerprinted bundles, so a stale copy would
    // keep pointing at a build that is no longer there.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CONTENT_SECURITY_POLICY));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    response
}

/// How long a browser may keep a file.
///
/// Vite fingerprints what it puts in `assets/` (`index-X9yN6mCH.js`), so those
/// names never change content and are kept for a year. Anything else - the
/// mark - keeps a short leash, because its name says nothing about its
/// contents.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use super::{Assets, cache_control, serve};

    /// Whether this binary carries a built interface at all.
    ///
    /// The suite runs both ways - a developer who has never run `pnpm build`,
    /// and CI, which builds it first - so the assertions say which case they
    /// are in rather than accepting either outcome for the same input.
    fn built() -> bool {
        Assets::get("index.html").is_some()
    }

    fn header_of(response: &axum::response::Response, name: header::HeaderName) -> String {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn fingerprinted_bundles_are_cached_for_good_and_nothing_else_is() {
        // The failure this guards is invisible and lasts a year: caching a
        // file whose name does not change with its content pins a browser to
        // a build that has been replaced.
        assert_eq!(cache_control("assets/index-X9yN6mCH.js"), "public, max-age=31536000, immutable");
        assert_eq!(cache_control("favicon.svg"), "public, max-age=3600");
    }

    #[tokio::test]
    async fn a_route_of_the_app_is_answered_with_the_document() {
        // A refresh on `/entries` must not 404: the server has no such file,
        // and only the app knows the route.
        let response = serve("/entries?account=x".parse().unwrap()).await;
        if built() {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(header_of(&response, header::CONTENT_TYPE), "text/html; charset=utf-8");
            assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-cache");
        } else {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(header_of(&response, header::CONTENT_TYPE), "text/plain; charset=utf-8");
        }
    }

    #[tokio::test]
    async fn the_document_may_not_be_framed_or_load_from_elsewhere() {
        let response = serve("/".parse().unwrap()).await;
        if !built() {
            eprintln!("skipped: no web interface is embedded in this binary");
            return;
        }
        let policy = header_of(&response, header::CONTENT_SECURITY_POLICY);
        assert!(policy.contains("frame-ancestors 'none'"), "the document can be framed: {policy}");
        assert!(policy.contains("script-src 'self'"), "scripts are not held to this origin: {policy}");
    }

    #[tokio::test]
    async fn an_unknown_api_path_is_a_404_and_not_the_app() {
        // A client calling a path no service owns must be told so. Answered
        // with the app it would get a 200 and a page of HTML it cannot parse,
        // and the mistake would surface somewhere else entirely.
        for path in ["/api", "/api/", "/api/v1/portfolio/positions", "/api/v2/ledger/accounts"] {
            let response = serve(path.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path} was answered with the app");
        }
    }

    #[tokio::test]
    async fn a_bundle_is_served_with_its_own_type_and_cached_hard() {
        // Skipped rather than faked when there is no build: asserting on an
        // asset that does not exist would test the fallback a second time.
        let Some(name) = Assets::iter().find(|name| name.ends_with(".js")) else {
            eprintln!("skipped: no web interface is embedded in this binary");
            return;
        };

        let response = serve(format!("/{name}").parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = header_of(&response, header::CONTENT_TYPE);
        assert!(content_type.contains("javascript"), "a bundle served as `{content_type}` will not run");
        assert!(header_of(&response, header::CACHE_CONTROL).contains("immutable"));
    }
}
