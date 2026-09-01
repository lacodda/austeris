//! The identity service's HTTP surface.
//!
//! These paths are private to the compose network: the gateway is what the
//! world reaches, and it forwards `/api/v1/auth/...` here (ADR 0001).

use anyhow::Result;
use austeris_common::{AppError, AppResult, health};
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{MIGRATOR, password, session};

/// What every handler here needs.
#[derive(Clone)]
pub struct ServiceState {
    pool: PgPool,
    /// Whether to mark the session cookie `Secure`.
    ///
    /// Unconditional would silently break every sign-in on an installation
    /// served over plain HTTP, which a self-hosted product on a home network
    /// often is.
    secure_cookies: bool,
}

/// Builds the service's router.
pub fn router(pool: PgPool, secure_cookies: bool) -> Router {
    let state = ServiceState {
        pool: pool.clone(),
        secure_cookies,
    };

    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool, &MIGRATOR))))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/logout-everywhere", post(logout_everywhere))
        .route("/auth/me", get(me))
        .with_state(state)
}

/// What a sign-in carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Credentials {
    /// The address, matched case-insensitively.
    pub email: String,
    /// The password, never stored or logged.
    pub password: String,
}

/// Who the caller is, as `/auth/me` reports it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Identity {
    /// The user id other services are told about.
    pub id: Uuid,
    /// The address, in the spelling it was created with.
    pub email: String,
    /// What to show in an interface.
    pub display_name: String,
}

/// Signs in, or refuses without saying which half was wrong.
///
/// An unknown address, a deactivated account and a wrong password are one
/// answer: distinguishing them turns this form into a way to learn who has an
/// account here.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = Credentials,
    responses(
        (status = 200, description = "Signed in; the session arrives as a cookie"),
        (status = 401, description = "Wrong email or password", body = austeris_common::error::ErrorBody),
        (status = 429, description = "Too many failed attempts against this address", body = austeris_common::error::ErrorBody),
    ),
)]
async fn login(State(state): State<ServiceState>, Json(credentials): Json<Credentials>) -> AppResult<Response> {
    // Refusing early: an address under a run of failed guesses stops being
    // worth guessing at, whether or not it is a real account here.
    if session::is_locked_out(&state.pool, &credentials.email).await? {
        tracing::warn!("a sign-in was refused because the address is locked out");
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            anyhow::anyhow!("too many failed attempts; try again in {} minutes", session::LOCKOUT_WINDOW_MINUTES),
        ));
    }

    let row: Option<(Uuid, Option<String>)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE lower(email) = lower($1) AND active")
        .bind(&credentials.email)
        .fetch_optional(&state.pool)
        .await?;

    // An unknown address, a deactivated account and a wrong password are one
    // answer. Distinguishing them turns the sign-in form into a way to learn
    // who has an account here.
    let refused = || AppError::new(StatusCode::UNAUTHORIZED, anyhow::anyhow!("wrong email or password"));

    let Some((user_id, Some(hash))) = row else {
        // An account with no password cannot be signed into - and the work to
        // verify one is done anyway, so that "no such user" and "wrong
        // password" do not differ by a measurable pause.
        // The result is deliberately discarded: this call exists for the time
        // it takes, not for its answer.
        let _ = password::verify(&credentials.password, password::DUMMY_HASH);
        session::record_failure(&state.pool, &credentials.email).await;
        return Err(refused());
    };

    if !password::verify(&credentials.password, &hash) {
        session::record_failure(&state.pool, &credentials.email).await;
        return Err(refused());
    }

    let issued = session::issue(&state.pool, user_id).await?;
    session::clear_failures(&state.pool, &credentials.email).await?;
    tracing::info!(%user_id, "signed in");

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie_for(&issued.token, state.secure_cookies))],
        Json(serde_json::json!({"status": "ok"})),
    )
        .into_response())
}

/// Signs out of this session only.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "This session is over; the token stops working at once"),
        (status = 401, description = "Not signed in", body = austeris_common::error::ErrorBody),
    ),
)]
async fn logout(State(state): State<ServiceState>, holder: Holder) -> AppResult<Response> {
    session::revoke(&state.pool, holder.0.session_id).await?;
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, expired_cookie(state.secure_cookies))],
        Json(serde_json::json!({"status": "ok"})),
    )
        .into_response())
}

/// Signs out everywhere - the answer to a laptop left on a train.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout-everywhere",
    tag = "auth",
    responses(
        (status = 200, description = "Every session is over; the account is untouched"),
        (status = 401, description = "Not signed in", body = austeris_common::error::ErrorBody),
    ),
)]
async fn logout_everywhere(State(state): State<ServiceState>, holder: Holder) -> AppResult<Response> {
    let ended = session::revoke_all(&state.pool, holder.0.user_id).await?;
    tracing::info!(user_id = %holder.0.user_id, ended, "ended every session");
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, expired_cookie(state.secure_cookies))],
        Json(serde_json::json!({"status": "ok", "ended": ended})),
    )
        .into_response())
}

/// Who am I - what an interface calls on load to decide whether to show a form.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "The signed-in person", body = Identity),
        (status = 401, description = "Not signed in", body = austeris_common::error::ErrorBody),
    ),
)]
async fn me(holder: Holder) -> AppResult<Json<Identity>> {
    let holder = holder.0;
    Ok(Json(Identity {
        id: holder.user_id,
        email: holder.email,
        display_name: holder.display_name,
    }))
}

/// An authenticated caller.
///
/// Handlers take it as an argument, which makes the check impossible to forget:
/// a route without it simply has no session to act on.
pub struct Holder(pub session::Holder);

impl axum::extract::FromRequestParts<ServiceState> for Holder {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &ServiceState) -> Result<Self, Self::Rejection> {
        let unauthorized = || AppError::new(StatusCode::UNAUTHORIZED, anyhow::anyhow!("not signed in"));

        let token = session_cookie(parts).ok_or_else(unauthorized)?;
        session::authenticate(&state.pool, &token).await?.map(Holder).ok_or_else(unauthorized)
    }
}

/// Pulls the session token out of the request's cookies.
fn session_cookie(parts: &axum::http::request::Parts) -> Option<String> {
    let header = parts.headers.get(header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == session::COOKIE).then(|| value.trim().to_owned())
    })
}

/// Builds the session cookie.
///
/// `HttpOnly` so no script can read the token even if one is injected;
/// `SameSite=Strict` because every caller is our own page on our own origin,
/// which also makes CSRF tokens unnecessary; `Secure` unless the operator is on
/// plain HTTP, where an unconditional flag would break every sign-in silently.
fn cookie_for(token: &str, secure: bool) -> HeaderValue {
    let max_age = session::LIFETIME_DAYS * 24 * 60 * 60;
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}",
        session::COOKIE
    ))
    .expect("the cookie is built from hex and ASCII literals")
}

/// The same cookie, already expired: what tells the browser to forget it.
fn expired_cookie(secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}", session::COOKIE))
        .expect("the cookie is built from ASCII literals")
}

/// Creates the first person, if this installation has none.
///
/// The alternative - refusing to start until the operator sets a variable -
/// makes the first run a documentation exercise, and the password it teaches
/// them to write then lives in a compose file forever. Here the secret exists
/// for one line of one log.
///
/// Does nothing once anyone exists, so a restart is not a way to mint
/// credentials and nothing is printed on the hundredth boot.
///
/// # Errors
///
/// Returns an error when the account cannot be read or created.
pub async fn ensure_first_user(pool: &PgPool, email: &str) -> Result<Option<String>> {
    use rand::RngExt;

    // No `l`, `1`, `0` or `o`: this is read off a terminal and typed once.
    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyz23456789";

    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE active").fetch_one(pool).await?;
    if existing > 0 {
        return Ok(None);
    }

    // Twenty characters: long enough that the lockout is never the thing
    // standing between an attacker and the account.
    let mut rng = rand::rng();
    let password: String = (0..20).map(|_| char::from(ALPHABET[rng.random_range(0..ALPHABET.len())])).collect();

    sqlx::query("INSERT INTO users (email, display_name, password_hash) VALUES ($1, $1, $2)")
        .bind(email)
        .bind(password::hash(&password)?)
        .execute(pool)
        .await?;

    Ok(Some(password))
}

#[cfg(test)]
mod tests {
    use axum::http::{Request, header};

    use super::{cookie_for, expired_cookie, session_cookie};

    fn parts_with_cookie(value: &str) -> axum::http::request::Parts {
        Request::builder().header(header::COOKIE, value).body(()).unwrap().into_parts().0
    }

    #[test]
    fn the_session_token_is_found_among_other_cookies() {
        let parts = parts_with_cookie("theme=dark; austeris_session=abc123; locale=en");
        assert_eq!(session_cookie(&parts).as_deref(), Some("abc123"));
    }

    #[test]
    fn a_request_without_the_cookie_carries_no_token() {
        assert_eq!(session_cookie(&parts_with_cookie("theme=dark")), None);
        // A cookie whose name merely ends the same way is not ours.
        assert_eq!(session_cookie(&parts_with_cookie("not_austeris_session=abc")), None);
    }

    #[test]
    fn the_cookie_cannot_be_read_by_a_script_or_sent_across_origins() {
        let cookie = cookie_for("abc", true);
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Strict"), "{cookie}");
        assert!(cookie.contains("; Secure"), "{cookie}");
    }

    #[test]
    fn plain_http_gets_a_cookie_without_secure() {
        // An unconditional `Secure` makes every sign-in fail on an
        // installation served over HTTP, with nothing in the response saying
        // why - the browser simply drops the cookie.
        let cookie = cookie_for("abc", false);
        assert!(!cookie.to_str().unwrap().contains("Secure"));
    }

    #[test]
    fn signing_out_expires_the_cookie_rather_than_reissuing_it() {
        let cookie = expired_cookie(true);
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.contains("Max-Age=0"), "{cookie}");
        assert!(cookie.starts_with("austeris_session=;"), "the value must be cleared: {cookie}");
    }
}

/// This service's share of the platform's `OpenAPI` document.
///
/// The paths are the public ones - what a client calls through the gateway -
/// rather than the internal ones this router listens on. A document describing
/// `/auth/login` would be accurate about the compose network and useless to
/// everyone outside it.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(login, logout, logout_everywhere, me),
    components(schemas(Credentials, Identity)),
    tags((name = "auth", description = "Signing in, signing out, and asking who you are")),
)]
pub struct ApiDoc;
