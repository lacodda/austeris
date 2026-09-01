//! Instruments and prices against a real PostgreSQL.
//!
//! What matters here lives in the database: that a price survives storage with
//! every digit intact, that priority decides which source answers, and that "as
//! of" means the newest observation *before* an instant rather than the nearest
//! one. Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job
//! that owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Config, db};
// The service is exercised through its own trait rather than over a socket:
// what is worth testing is the contract's answers, not tonic's transport.
use austeris_proto::market::v1::market_server::Market as _;

use austeris_market::model::Kind;
use austeris_market::routes::Sources;
use austeris_market::{MIGRATOR, repository, routes};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::{TimeZone, Utc};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
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

/// A decimal from its digits, which is the only way to write one exactly.
fn decimal(digits: &str) -> Decimal {
    Decimal::from_str(digits).expect("a decimal")
}

/// Calls the service and returns the status and body.
async fn call(pool: &PgPool, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, String) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(json) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .unwrap();

    let response = routes::router(pool.clone(), Sources::from_env()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn a_price_survives_storage_with_every_digit() {
    let Some(pool) = pool("test_market_precision").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    // Eighteen decimal places, which is what the column promises and what a
    // token with 18 decimals actually needs.
    let exact = decimal("61234.567890123456789012");
    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, exact, "test").await.unwrap();

    let stored = repository::price_at(&pool, btc.id, "USD", observed).await.unwrap().expect("the price");
    // NUMERIC(38, 18) keeps eighteen places; the input carries a few more, so
    // what comes back is the rounded value - and that rounding is the column's
    // promise, not a float's silent drift.
    assert_eq!(stored.price, decimal("61234.567890123456789012").round_dp(18));
    assert_eq!(stored.price.to_string(), "61234.567890123456789012");
}

#[tokio::test]
async fn priority_decides_which_source_answers() {
    let Some(pool) = pool("test_market_priority").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    repository::bind_source(&pool, btc.id, "preferred", "x", 10).await.unwrap();
    repository::bind_source(&pool, btc.id, "fallback", "y", 50).await.unwrap();

    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, decimal("100"), "fallback")
        .await
        .unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, decimal("200"), "preferred")
        .await
        .unwrap();

    let price = repository::price_at(&pool, btc.id, "USD", observed).await.unwrap().expect("a price");
    assert_eq!(price.source, "preferred", "the lower priority number must win");
    assert_eq!(price.price, decimal("200"));
}

#[tokio::test]
async fn a_newer_observation_beats_a_better_ranked_stale_one() {
    let Some(pool) = pool("test_market_recency").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    repository::bind_source(&pool, btc.id, "preferred", "x", 10).await.unwrap();
    repository::bind_source(&pool, btc.id, "fallback", "y", 50).await.unwrap();

    let morning = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let noon = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", morning, decimal("100"), "preferred")
        .await
        .unwrap();
    repository::record_price(&pool, btc.id, "USD", noon, decimal("200"), "fallback").await.unwrap();

    // This is the whole point of a fallback: the preferred source went quiet at
    // nine, and a three-hour-old price is worse than a current one from the
    // second-choice source.
    let price = repository::price_at(&pool, btc.id, "USD", noon).await.unwrap().expect("a price");
    assert_eq!(price.source, "fallback");
    assert_eq!(price.price, decimal("200"));
}

#[tokio::test]
async fn as_of_means_before_rather_than_nearest() {
    let Some(pool) = pool("test_market_as_of").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    let morning = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let evening = Utc.with_ymd_and_hms(2026, 9, 1, 21, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", morning, decimal("100"), "test").await.unwrap();
    repository::record_price(&pool, btc.id, "USD", evening, decimal("300"), "test").await.unwrap();

    // Asked about noon: the evening price is nearer in time but had not been
    // observed yet. Valuing a trade with a price from after it is how a
    // portfolio ends up worth what it never was.
    let noon = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let price = repository::price_at(&pool, btc.id, "USD", noon).await.unwrap().expect("a price");
    assert_eq!(price.price, decimal("100"));

    // And nothing at all before the first observation.
    let dawn = Utc.with_ymd_and_hms(2026, 9, 1, 5, 0, 0).unwrap();
    assert!(repository::price_at(&pool, btc.id, "USD", dawn).await.unwrap().is_none());
}

#[tokio::test]
async fn prices_are_kept_per_currency() {
    let Some(pool) = pool("test_market_currency").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, decimal("61000"), "test")
        .await
        .unwrap();
    repository::record_price(&pool, btc.id, "PYG", observed, decimal("445300000"), "test")
        .await
        .unwrap();

    let usd = repository::price_at(&pool, btc.id, "USD", observed).await.unwrap().expect("USD");
    let pyg = repository::price_at(&pool, btc.id, "PYG", observed).await.unwrap().expect("PYG");
    assert_eq!(usd.price, decimal("61000"));
    assert_eq!(pyg.price, decimal("445300000"), "a price in one currency must not answer for another");
}

#[tokio::test]
async fn a_batch_answers_for_each_instrument_and_omits_the_unpriced() {
    let Some(pool) = pool("test_market_batch").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();
    let eth = repository::upsert_instrument(&pool, Kind::Crypto, "ETH", "Ethereum", Some(18)).await.unwrap();
    let unpriced = repository::upsert_instrument(&pool, Kind::Crypto, "XMR", "Monero", Some(12)).await.unwrap();

    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, decimal("61000"), "test")
        .await
        .unwrap();
    repository::record_price(&pool, eth.id, "USD", observed, decimal("2456.78"), "test")
        .await
        .unwrap();

    let prices = repository::latest_prices(&pool, &[btc.id, eth.id, unpriced.id], "USD").await.unwrap();
    assert_eq!(prices.len(), 2, "the unpriced instrument is absent, not an error");
    assert!(prices.iter().any(|p| p.instrument_id == btc.id));
    assert!(prices.iter().any(|p| p.instrument_id == eth.id));
}

#[tokio::test]
async fn history_is_bounded_by_its_window_and_ordered_oldest_first() {
    let Some(pool) = pool("test_market_history").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    for (hour, price) in [(9, "100"), (12, "200"), (15, "300"), (18, "400")] {
        let at = Utc.with_ymd_and_hms(2026, 9, 1, hour, 0, 0).unwrap();
        repository::record_price(&pool, btc.id, "USD", at, decimal(price), "test").await.unwrap();
    }

    let from = Utc.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap();
    let to = Utc.with_ymd_and_hms(2026, 9, 1, 16, 0, 0).unwrap();
    let history = repository::price_history(&pool, btc.id, "USD", from, to).await.unwrap();

    assert_eq!(history.len(), 2, "only the observations inside the window");
    assert_eq!(history[0].price, decimal("200"), "oldest first");
    assert_eq!(history[1].price, decimal("300"));
}

#[tokio::test]
async fn an_instrument_is_created_once_however_often_it_is_synced() {
    let Some(pool) = pool("test_market_idempotent").await else { return };

    let first = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();
    let again = repository::upsert_instrument(&pool, Kind::Crypto, "btc", "Bitcoin (renamed)", Some(8))
        .await
        .unwrap();

    // Same id: everything that references an instrument keeps working, and the
    // symbol is matched without regard to case.
    assert_eq!(first.id, again.id);
    assert_eq!(again.name, "Bitcoin (renamed)", "a later sync updates what it knows");
    assert_eq!(repository::instruments(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn history_survives_a_source_being_unbound() {
    let Some(pool) = pool("test_market_unbound").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    repository::bind_source(&pool, btc.id, "retired", "x", 10).await.unwrap();
    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", observed, decimal("100"), "retired")
        .await
        .unwrap();

    sqlx::query("DELETE FROM instrument_sources WHERE instrument_id = $1")
        .bind(btc.id)
        .execute(&pool)
        .await
        .unwrap();

    // Removing a binding says "stop asking this source", not "forget what it
    // told us". A portfolio valued last year must still be valuable today.
    let price = repository::price_at(&pool, btc.id, "USD", observed).await.unwrap();
    assert!(price.is_some(), "the history vanished with the binding");
}

#[tokio::test]
async fn the_rest_surface_creates_reads_and_prices_an_instrument() {
    let Some(pool) = pool("test_market_rest").await else { return };

    let (status, body) = call(
        &pool,
        "POST",
        "/market/instruments",
        Some(r#"{"kind":"crypto","symbol":"BTC","name":"Bitcoin","decimals":8}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = created["id"].as_str().expect("an id").to_owned();

    let (status, body) = call(&pool, "GET", &format!("/market/instruments/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Bitcoin"), "{body}");

    let observed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, id.parse().unwrap(), "USD", observed, decimal("61234.5"), "test")
        .await
        .unwrap();

    let (status, body) = call(&pool, "GET", "/market/prices", None).await;
    assert_eq!(status, StatusCode::OK);
    // A string in JSON, not a number: a JavaScript client parsing a number gets
    // an IEEE double and has lost the value before rendering it (ADR 0004).
    // The trailing zeros are the column's scale coming back with the value -
    // NUMERIC(38, 18) stores 61234.5 as eighteen decimal places, and reporting
    // the scale it was stored at is more honest than trimming it here.
    assert!(body.contains(r#""price":"61234.500000000000000000""#), "the price is not a JSON string: {body}");
    assert!(!body.contains(r#""price":61234"#), "the price must never be a JSON number: {body}");
}

#[tokio::test]
async fn syncing_is_matched_before_an_instrument_id() {
    let Some(pool) = pool("test_market_route_order").await else { return };

    // `/instruments/sync` and `/instruments/{id}` overlap; if the parameter
    // route wins, `sync` is read as an id and the endpoint is unreachable.
    // Without a key the source is off, so a 503 here proves the sync handler
    // was reached - a 400 or 404 would mean it was not.
    let (status, body) = call(&pool, "POST", "/market/instruments/sync", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert!(body.contains("AUSTERIS_CMC_API_KEY"), "{body}");
}

#[tokio::test]
async fn a_source_cannot_be_bound_to_an_instrument_that_does_not_exist() {
    let Some(pool) = pool("test_market_bind_missing").await else { return };

    let missing = Uuid::new_v4();
    let (status, _) = call(
        &pool,
        "POST",
        &format!("/market/instruments/{missing}/sources"),
        Some(r#"{"source":"coinmarketcap","external_id":"1"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a foreign-key error would have been a 500");
}

#[tokio::test]
async fn a_backwards_window_is_refused_rather_than_answered_empty() {
    let Some(pool) = pool("test_market_bad_window").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();

    let uri = format!("/market/prices/{}/history?from=2026-09-02T00:00:00Z&to=2026-09-01T00:00:00Z", btc.id);
    let (status, _) = call(&pool, "GET", &uri, None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an empty list would look like `no prices` rather than `bad question`"
    );
}

#[tokio::test]
async fn readiness_reports_the_schema_version_this_build_expects() {
    let Some(pool) = pool("test_market_readyz").await else { return };

    let (status, body) = call(&pool, "GET", "/readyz", None).await;
    assert_eq!(status, StatusCode::OK);
    let newest = MIGRATOR.iter().map(|m| m.version).max().expect("the service ships migrations");
    assert!(body.contains(&newest.to_string()), "{body}");
}

#[tokio::test]
async fn the_grpc_contract_answers_with_the_same_rule_as_rest() {
    let Some(pool) = pool("test_market_grpc").await else { return };
    let btc = repository::upsert_instrument(&pool, Kind::Crypto, "BTC", "Bitcoin", Some(8)).await.unwrap();
    let eth = repository::upsert_instrument(&pool, Kind::Crypto, "ETH", "Ethereum", Some(18)).await.unwrap();

    let morning = Utc.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
    let noon = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    repository::record_price(&pool, btc.id, "USD", morning, decimal("60000"), "test").await.unwrap();
    repository::record_price(&pool, btc.id, "USD", noon, decimal("61234.5"), "test").await.unwrap();

    let service = austeris_market::grpc::Service::for_tests(pool.clone());

    let response = service
        .get_prices(tonic::Request::new(austeris_proto::market::v1::GetPricesRequest {
            instrument_ids: vec![btc.id.to_string(), eth.id.to_string()],
            quote_currency: "USD".to_owned(),
        }))
        .await
        .expect("asking for prices")
        .into_inner();

    assert_eq!(response.prices.len(), 1, "the unpriced instrument is absent, not an error");
    let price = &response.prices[0];
    // A string on the wire, because protobuf has no decimal type (ADR 0004).
    assert_eq!(price.price, "61234.500000000000000000");
    assert_eq!(price.observed_at_unix_seconds, noon.timestamp());

    // "As of" reaches back, exactly as the REST surface does.
    let response = service
        .get_price_at(tonic::Request::new(austeris_proto::market::v1::GetPriceAtRequest {
            instrument_id: btc.id.to_string(),
            quote_currency: "USD".to_owned(),
            at_unix_seconds: Utc.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap().timestamp(),
        }))
        .await
        .expect("asking for a price")
        .into_inner();

    let price = response.price.expect("a price before ten in the morning");
    assert_eq!(price.price, "60000.000000000000000000", "the noon price had not happened yet");
}

#[tokio::test]
async fn an_unparseable_id_is_the_callers_mistake_not_an_empty_answer() {
    let Some(pool) = pool("test_market_grpc_bad_id").await else { return };

    let service = austeris_market::grpc::Service::for_tests(pool);

    let status = service
        .get_prices(tonic::Request::new(austeris_proto::market::v1::GetPricesRequest {
            instrument_ids: vec!["not-a-uuid".to_owned()],
            quote_currency: "USD".to_owned(),
        }))
        .await
        .expect_err("an unparseable id");

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
    assert!(status.message().contains("not-a-uuid"), "{}", status.message());
}
