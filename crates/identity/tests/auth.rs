//! Signing in, out, and being refused - against a real PostgreSQL.
//!
//! The behaviour worth testing here lives in the database: which addresses are
//! indistinguishable from each other, when a lockout takes effect, what a
//! session still authenticates after it is revoked. A pool that never connects
//! exercises none of it. Without `AUSTERIS_DATABASE_URL` these skip themselves,
//! and the CI job that owns them fails when they do.

use std::time::Duration;

use austeris_common::{Config, db};
use austeris_identity::{MIGRATOR, password, routes, session};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// Opens a pool on a schema of this test's own, migrated from empty.
async fn pool(schema: &str) -> Option<PgPool> {
    let Ok(database_url) = std::env::var("AUSTERIS_DATABASE_URL") else {
        eprintln!("{SKIP}");
        return None;
    };

    let config = Config {
        database_url: Some(database_url),
        bind: String::new(),
        max_connections: 4,
        acquire_timeout: Duration::from_secs(10),
    };

    // Each test owns a schema, so they run in parallel without one's rollback
    // erasing another's rows.
    let pool = db::connect(&config, schema).await.expect("connecting to the test database");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA \"{schema}\" CASCADE")))
        .execute(&pool)
        .await
        .expect("dropping the test schema");
    pool.close().await;

    let pool = db::connect(&config, schema).await.expect("recreating the test schema");
    austeris_common::migrate::run(&pool, &MIGRATOR).await.expect("migrating");
    Some(pool)
}

/// Creates a person who can sign in.
async fn user_with_password(pool: &PgPool, email: &str, plaintext: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO users (email, display_name, password_hash) VALUES ($1, $1, $2) RETURNING id")
        .bind(email)
        .bind(password::hash(plaintext).expect("hashing"))
        .fetch_one(pool)
        .await
        .expect("creating the user")
}

/// Posts credentials at the service and returns what came back.
async fn sign_in(pool: &PgPool, email: &str, password: &str) -> (StatusCode, Option<String>) {
    let request = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({"email": email, "password": password}).to_string()))
        .unwrap();

    let response = routes::router(pool.clone(), false).oneshot(request).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(std::borrow::ToOwned::to_owned);
    (status, cookie)
}

/// The token out of a `Set-Cookie` header.
fn token_from(cookie: &str) -> String {
    cookie
        .split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, value)| value.to_owned())
        .expect("a session cookie")
}

/// Calls an endpoint carrying a session.
async fn with_session(pool: &PgPool, method: &str, uri: &str, token: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, format!("{}={token}", session::COOKIE))
        .body(Body::empty())
        .unwrap();

    let response = routes::router(pool.clone(), false).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn a_correct_password_returns_a_session_that_names_its_owner() {
    let Some(pool) = pool("test_auth_login").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    let (status, cookie) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK);
    let cookie = cookie.expect("a session cookie");
    assert!(cookie.contains("HttpOnly"), "{cookie}");

    let (status, body) = with_session(&pool, "GET", "/auth/me", &token_from(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("owner@example.test"), "{body}");
}

#[tokio::test]
async fn the_address_is_matched_without_regard_to_case() {
    let Some(pool) = pool("test_auth_case").await else { return };
    user_with_password(&pool, "Owner@Example.test", "a good long password").await;

    let (status, _) = sign_in(&pool, "owner@EXAMPLE.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK, "an address typed in another case is the same address");
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_address_are_the_same_answer() {
    let Some(pool) = pool("test_auth_refusal").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    let (wrong, _) = sign_in(&pool, "owner@example.test", "not the password").await;
    let (unknown, _) = sign_in(&pool, "nobody@example.test", "not the password").await;

    // Distinguishing them turns the sign-in form into a way to learn who has an
    // account here.
    assert_eq!(wrong, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_deactivated_account_cannot_sign_in() {
    let Some(pool) = pool("test_auth_inactive").await else { return };
    let user_id = user_with_password(&pool, "gone@example.test", "a good long password").await;
    sqlx::query("UPDATE users SET active = false WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _) = sign_in(&pool, "gone@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_session_stops_working_the_moment_it_is_revoked() {
    let Some(pool) = pool("test_auth_logout").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    let (_, cookie) = sign_in(&pool, "owner@example.test", "a good long password").await;
    let token = token_from(&cookie.unwrap());

    let (status, _) = with_session(&pool, "POST", "/auth/logout", &token).await;
    assert_eq!(status, StatusCode::OK);

    // The whole reason sessions are server-side rather than signed tokens.
    let (status, _) = with_session(&pool, "GET", "/auth/me", &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a revoked session still authenticated");
}

#[tokio::test]
async fn signing_out_everywhere_ends_every_session_but_leaves_the_account() {
    let Some(pool) = pool("test_auth_logout_all").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    let (_, first) = sign_in(&pool, "owner@example.test", "a good long password").await;
    let (_, second) = sign_in(&pool, "owner@example.test", "a good long password").await;
    let first = token_from(&first.unwrap());
    let second = token_from(&second.unwrap());

    let (status, _) = with_session(&pool, "POST", "/auth/logout-everywhere", &first).await;
    assert_eq!(status, StatusCode::OK);

    for token in [&first, &second] {
        let (status, _) = with_session(&pool, "GET", "/auth/me", token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "a session survived signing out everywhere");
    }

    // Signing in again must still work: this ends sessions, not the account.
    let (status, _) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_expired_session_authenticates_nobody_even_before_it_is_swept() {
    let Some(pool) = pool("test_auth_expiry").await else { return };
    let user_id = user_with_password(&pool, "owner@example.test", "a good long password").await;

    let (_, cookie) = sign_in(&pool, "owner@example.test", "a good long password").await;
    let token = token_from(&cookie.unwrap());

    // Backdate it rather than wait a fortnight. The row is still there, which
    // is the point: expiry is enforced by the query, not by the sweep.
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second' WHERE user_id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, _) = with_session(&pool, "GET", "/auth/me", &token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let swept = session::sweep_expired(&pool).await.unwrap();
    assert_eq!(swept, 1, "the sweep should have found the expired row");
}

#[tokio::test]
async fn repeated_guesses_lock_the_address_out() {
    let Some(pool) = pool("test_auth_lockout").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    for _ in 0..session::LOCKOUT_THRESHOLD {
        let (status, _) = sign_in(&pool, "owner@example.test", "wrong").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // Even the right password is refused now - otherwise the lockout is a
    // suggestion rather than a limit.
    let (status, _) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn an_address_with_no_account_locks_out_the_same_way() {
    let Some(pool) = pool("test_auth_lockout_unknown").await else { return };

    for _ in 0..session::LOCKOUT_THRESHOLD {
        sign_in(&pool, "nobody@example.test", "wrong").await;
    }

    // If only real accounts locked out, the lockout itself would answer the
    // question the refusal refuses to answer.
    let (status, _) = sign_in(&pool, "nobody@example.test", "wrong").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn one_address_being_guessed_at_does_not_lock_out_another() {
    let Some(pool) = pool("test_auth_lockout_scope").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    for _ in 0..session::LOCKOUT_THRESHOLD {
        sign_in(&pool, "victim@example.test", "wrong").await;
    }

    let (status, _) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK, "an unrelated address was locked out too");
}

#[tokio::test]
async fn signing_in_forgets_the_failures_before_it() {
    let Some(pool) = pool("test_auth_lockout_reset").await else { return };
    user_with_password(&pool, "owner@example.test", "a good long password").await;

    // One short of the threshold, then a success.
    for _ in 0..session::LOCKOUT_THRESHOLD - 1 {
        sign_in(&pool, "owner@example.test", "wrong").await;
    }
    let (status, _) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK);

    // Without the reset, a single typo tomorrow would lock the account.
    for _ in 0..session::LOCKOUT_THRESHOLD - 1 {
        sign_in(&pool, "owner@example.test", "wrong").await;
    }
    let (status, _) = sign_in(&pool, "owner@example.test", "a good long password").await;
    assert_eq!(status, StatusCode::OK, "old failures were still being counted");
}

#[tokio::test]
async fn the_first_user_is_created_once_and_only_when_there_is_none() {
    let Some(pool) = pool("test_auth_bootstrap").await else { return };

    let password = routes::ensure_first_user(&pool, "owner@example.test")
        .await
        .expect("creating the first user")
        .expect("a generated password");
    assert!(password.chars().count() >= 16, "the generated password is short: {}", password.chars().count());

    // A restart must not mint credentials, and must not print anything.
    let again = routes::ensure_first_user(&pool, "owner@example.test").await.expect("second call");
    assert!(again.is_none(), "a second call created another account or reset the password");

    // The password that was printed is the one that works.
    let (status, _) = sign_in(&pool, "owner@example.test", &password).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn without_a_session_nothing_private_answers() {
    let Some(pool) = pool("test_auth_anonymous").await else { return };

    for (method, uri) in [("GET", "/auth/me"), ("POST", "/auth/logout"), ("POST", "/auth/logout-everywhere")] {
        let request = Request::builder().method(method).uri(uri).body(Body::empty()).unwrap();
        let response = routes::router(pool.clone(), false).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method} {uri} answered without a session");
    }
}

#[tokio::test]
async fn readiness_reports_the_schema_version_this_build_expects() {
    let Some(pool) = pool("test_auth_readyz").await else { return };

    let request = Request::builder().uri("/readyz").body(Body::empty()).unwrap();
    let response = routes::router(pool, false).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    let newest = MIGRATOR.iter().map(|m| m.version).max().expect("the service ships migrations");
    assert!(body.contains(&newest.to_string()), "readiness did not name the applied version: {body}");
}
