//! Money that became something a module holds, and the entries a module keeps,
//! against a real PostgreSQL.
//!
//! A trade is the truth about its money: the entry it posts says what the
//! trade says, and only the module keeping it changes or takes it back. These
//! show the rules where they hold - in the schema and in the contract - for a
//! person typing, for the API refusing, and for a module restating.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Actor, Config, USER_HEADER, db};
use austeris_ledger::model::{AccountKind, Flow};
use austeris_ledger::{MIGRATOR, grpc, repository, routes};
use austeris_proto::ledger::v1::ledger_service_server::LedgerService as _;
use austeris_proto::ledger::v1::{Holding, Line as ProtoLine, PostEntryRequest, RestateEntryRequest, WithdrawEntryRequest, line::Side as ProtoSide};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;
use tonic::Code;
use tower::ServiceExt;
use uuid::Uuid;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// The key the module posts its trade under.
const KEY: &str = "portfolio:trade:0001";

/// Opens a pool on a schema of this test's own, migrated from empty.
async fn pool(schema: &str) -> Option<PgPool> {
    let Ok(database_url) = std::env::var("AUSTERIS_DATABASE_URL") else {
        eprintln!("{SKIP}");
        return None;
    };
    let config = Config {
        database_url: Some(database_url),
        bind: String::new(),
        max_connections: 8,
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

fn decimal(digits: &str) -> Decimal {
    Decimal::from_str(digits).expect("a decimal")
}

/// Calls the service as the gateway would, and reads JSON back.
async fn call(pool: &PgPool, owner: Uuid, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(USER_HEADER, owner.to_string())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
        .expect("a request");
    let response = routes::router(pool.clone()).oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    let text = String::from_utf8(bytes.to_vec()).expect("text");
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// A person with an exchange account kept in a stablecoin, opened through
/// the API as a person opens one.
async fn exchange(pool: &PgPool) -> (Uuid, Uuid) {
    let owner = Uuid::new_v4();
    let (status, account) = call(
        pool,
        owner,
        "POST",
        "/ledger/accounts",
        Some(json!({ "kind": "crypto_wallet", "name": "Exchange", "currency": "usdt", "opening_balance": "1000" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    assert_eq!(account["currency"], "USDT", "a stablecoin is a currency an account is kept in");
    (owner, account["id"].as_str().expect("an id").parse().expect("an id"))
}

fn account_line(account: Uuid, amount: &str) -> ProtoLine {
    ProtoLine {
        side: Some(ProtoSide::AccountId(account.to_string())),
        amount: amount.to_owned(),
        currency: "USDT".to_owned(),
        note: String::new(),
    }
}

fn holding_line(amount: &str) -> ProtoLine {
    ProtoLine {
        side: Some(ProtoSide::Holding(Holding {})),
        amount: amount.to_owned(),
        currency: "USDT".to_owned(),
        note: String::new(),
    }
}

/// A purchase of a coin, as a module posts it.
fn purchase(owner: Uuid, account: Uuid, kept: bool) -> PostEntryRequest {
    PostEntryRequest {
        owner_id: owner.to_string(),
        occurred_on: "2026-10-20".to_owned(),
        description: "Buy 0.01 BTC".to_owned(),
        idempotency_key: KEY.to_owned(),
        source: "portfolio".to_owned(),
        lines: vec![account_line(account, "-601"), holding_line("601")],
        kept,
        ..PostEntryRequest::default()
    }
}

/// What the account holds today, as its balance says.
async fn balance(pool: &PgPool, owner: Uuid) -> Decimal {
    let (status, balances) = call(pool, owner, "GET", "/ledger/balances", None).await;
    assert_eq!(status, StatusCode::OK);
    decimal(balances["accounts"][0]["amount"].as_str().expect("an amount"))
}

#[tokio::test]
async fn a_purchase_leaves_the_account_and_is_not_spending() {
    let Some(pool) = pool("test_holdings_purchase").await else { return };
    let (owner, account) = exchange(&pool).await;
    let ledger = grpc::Service::for_tests(pool.clone());

    let posted = ledger
        .post_entry(tonic::Request::new(purchase(owner, account, true)))
        .await
        .expect("posting a purchase")
        .into_inner();
    assert!(posted.created);

    assert_eq!(balance(&pool, owner).await, decimal("399"));
    let (_, entry) = call(&pool, owner, "GET", &format!("/ledger/entries/{}", posted.entry_id), None).await;
    assert_eq!(entry["kept"], true);
    assert_eq!(entry["source"], "portfolio");
    let sides: Vec<&str> = entry["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|line| line["side"].as_str().unwrap())
        .collect();
    assert_eq!(sides, ["account", "holding"]);

    // Nothing was spent: a report of what money went on has nothing in it.
    let (status, totals) = call(&pool, owner, "GET", "/ledger/totals?by=category", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(totals["groups"], json!([]), "a purchase was counted as spending");
}

#[tokio::test]
async fn a_holding_belongs_to_an_entry_a_module_keeps() {
    let Some(pool) = pool("test_holdings_unkept").await else { return };
    let (owner, account) = exchange(&pool).await;
    let ledger = grpc::Service::for_tests(pool.clone());

    let refused = ledger
        .post_entry(tonic::Request::new(purchase(owner, account, false)))
        .await
        .expect_err("a holding posted for anyone to delete");
    assert_eq!(refused.code(), Code::InvalidArgument, "{refused:?}");
    assert!(refused.message().contains("holdings"), "{}", refused.message());
    assert_eq!(balance(&pool, owner).await, decimal("1000"), "the refused entry left something behind");

    // An entry is kept by the module that posts it, so it has to say who.
    let mut nameless = purchase(owner, account, true);
    nameless.source = String::new();
    let refused = ledger.post_entry(tonic::Request::new(nameless)).await.expect_err("kept by nobody");
    assert_eq!(refused.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn a_person_changes_what_is_theirs_on_a_kept_entry_and_nothing_else() {
    let Some(pool) = pool("test_holdings_person").await else { return };
    let (owner, account) = exchange(&pool).await;
    let ledger = grpc::Service::for_tests(pool.clone());
    let id = ledger
        .post_entry(tonic::Request::new(purchase(owner, account, true)))
        .await
        .expect("posting")
        .into_inner()
        .entry_id;
    let entry = format!("/ledger/entries/{id}");

    let (status, refused) = call(&pool, owner, "DELETE", &entry, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "entry.kept");
    assert_eq!(refused["params"]["module"], "portfolio");

    for patch in [json!({ "occurred_on": "2026-10-21" }), json!({ "description": "lunch" })] {
        let (status, refused) = call(&pool, owner, "PATCH", &entry, Some(patch.clone())).await;
        assert_eq!(status, StatusCode::CONFLICT, "{patch} was let through: {refused}");
        assert_eq!(refused["code"], "entry.kept");
    }
    // Its labels are the person's, as on any entry.
    let (status, tagged) = call(&pool, owner, "PATCH", &entry, Some(json!({ "tags": ["long-term"] }))).await;
    assert_eq!(status, StatusCode::OK, "{tagged}");
    assert_eq!(tagged["tags"], json!(["long-term"]));

    // Recorded again here, it would be money gone into a coin no trade knows.
    let (status, refused) = call(&pool, owner, "POST", &format!("{entry}/repeat"), Some(json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "entry.kept");
    let (status, refused) = call(&pool, owner, "POST", "/ledger/templates", Some(json!({ "entry_id": id, "name": "BTC" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "entry.kept");

    // The schema holds it for a path that did not ask first.
    let mut transaction = Actor::Person(owner).begin(&pool).await.expect("a transaction");
    let error = sqlx::query("UPDATE entry_lines SET amount = -amount WHERE entry_id = $1::uuid")
        .bind(&id)
        .execute(&mut *transaction)
        .await
        .expect_err("a person rewrote a kept entry's money");
    assert_eq!(
        error.as_database_error().and_then(sqlx::error::DatabaseError::code).as_deref(),
        Some("AUK01"),
        "{error}"
    );
}

#[tokio::test]
async fn the_module_restates_and_takes_back_what_it_keeps() {
    let Some(pool) = pool("test_holdings_module").await else { return };
    let (owner, account) = exchange(&pool).await;
    let ledger = grpc::Service::for_tests(pool.clone());
    let id = ledger
        .post_entry(tonic::Request::new(purchase(owner, account, true)))
        .await
        .expect("posting")
        .into_inner()
        .entry_id;
    call(&pool, owner, "PATCH", &format!("/ledger/entries/{id}"), Some(json!({ "tags": ["long-term"] }))).await;

    let restate = |source: &str| RestateEntryRequest {
        owner_id: owner.to_string(),
        idempotency_key: KEY.to_owned(),
        source: source.to_owned(),
        occurred_on: "2026-10-21".to_owned(),
        description: "Buy 0.012 BTC".to_owned(),
        lines: vec![account_line(account, "-650.5"), holding_line("650.5")],
    };

    // Another module does not change what this one keeps.
    let refused = ledger
        .restate_entry(tonic::Request::new(restate("imports")))
        .await
        .expect_err("another module restated a trade");
    assert_eq!(refused.code(), Code::FailedPrecondition, "{refused:?}");

    let restated = ledger
        .restate_entry(tonic::Request::new(restate("portfolio")))
        .await
        .expect("restating")
        .into_inner();
    assert_eq!(restated.entry_id, id, "a restated trade is the same entry");
    assert_eq!(balance(&pool, owner).await, decimal("349.5"));
    let (_, entry) = call(&pool, owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(entry["occurred_on"], "2026-10-21");
    assert_eq!(entry["cleared_on"], "2026-10-21", "it is posted the day it happened, as before");
    assert_eq!(entry["description"], "Buy 0.012 BTC");
    assert_eq!(entry["tags"], json!(["long-term"]), "what the person added went with the restatement");

    // Its history says who changed what.
    let (_, history) = call(&pool, owner, "GET", &format!("/ledger/entries/{id}/history"), None).await;
    let last = history.as_array().and_then(|revisions| revisions.last()).expect("a revision");
    assert_eq!(last["action"], "changed");
    assert_eq!(last["who"], json!({ "kind": "module", "module": "portfolio" }));
    assert!(last["changed"].as_array().expect("fields").contains(&json!("lines")), "{last}");

    let withdraw = WithdrawEntryRequest {
        owner_id: owner.to_string(),
        idempotency_key: KEY.to_owned(),
        source: "portfolio".to_owned(),
    };
    let first = ledger.withdraw_entry(tonic::Request::new(withdraw.clone())).await.expect("withdrawing");
    assert!(first.into_inner().withdrawn);
    // A retry is answered, not refused.
    let again = ledger.withdraw_entry(tonic::Request::new(withdraw)).await.expect("withdrawing again");
    assert!(!again.into_inner().withdrawn);

    assert_eq!(balance(&pool, owner).await, decimal("1000"));
    // Only the module could bring it back, so the trash does not offer it.
    let (_, trash) = call(&pool, owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash["entries"], json!([]), "a trade's money waits in the trash for a person to restore");
    let (status, _) = call(&pool, owner, "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, history) = call(&pool, owner, "GET", &format!("/ledger/entries/{id}/history"), None).await;
    let last = history.as_array().and_then(|revisions| revisions.last()).expect("a revision");
    assert_eq!(last["action"], "deleted");
    assert_eq!(last["who"]["module"], "portfolio");

    // Restating what is gone is said, not invented.
    let missing = ledger
        .restate_entry(tonic::Request::new(restate("portfolio")))
        .await
        .expect_err("restated an entry that was taken back");
    assert_eq!(missing.code(), Code::NotFound);
}

#[tokio::test]
async fn an_entry_a_person_typed_is_not_a_modules_to_restate() {
    let Some(pool) = pool("test_holdings_typed").await else { return };
    let owner = Uuid::new_v4();
    let account = repository::create_account(&pool, owner, AccountKind::Cash, "Wallet", "USD", Decimal::ZERO)
        .await
        .expect("an account");
    let food = repository::create_category(&pool, owner, None, Flow::Expense, "food")
        .await
        .expect("a category");
    let ledger = grpc::Service::for_tests(pool.clone());
    let line = |side: ProtoSide, amount: &str| ProtoLine {
        side: Some(side),
        amount: amount.to_owned(),
        currency: "USD".to_owned(),
        note: String::new(),
    };
    // Posted by a module that does not keep it: a receipt, the person's now.
    ledger
        .post_entry(tonic::Request::new(PostEntryRequest {
            owner_id: owner.to_string(),
            occurred_on: "2026-10-20".to_owned(),
            idempotency_key: "imports:receipt".to_owned(),
            source: "imports".to_owned(),
            lines: vec![
                line(ProtoSide::AccountId(account.id.to_string()), "-12"),
                line(ProtoSide::CategoryId(food.id.to_string()), "12"),
            ],
            ..PostEntryRequest::default()
        }))
        .await
        .expect("posting a receipt");

    let refused = ledger
        .withdraw_entry(tonic::Request::new(WithdrawEntryRequest {
            owner_id: owner.to_string(),
            idempotency_key: "imports:receipt".to_owned(),
            source: "imports".to_owned(),
        }))
        .await
        .expect_err("a module took back an entry it gave the person");
    assert_eq!(refused.code(), Code::FailedPrecondition);
    assert_eq!(
        repository::balances(&pool, owner, chrono::NaiveDate::from_ymd_opt(2026, 10, 31).expect("a day"))
            .await
            .expect("balances")[0]
            .amount,
        decimal("-12")
    );
}

#[tokio::test]
async fn an_installation_with_entries_migrates_to_holdings() {
    let Some(pool) = pool("test_holdings_migrate").await else { return };
    austeris_common::migrate::undo(&pool, &MIGRATOR, 20_261_020_100_001)
        .await
        .expect("rolling back to v0.13");

    // Books as v0.13 wrote them: an entry, which the schema then knew nothing
    // about keeping.
    let owner = Uuid::new_v4();
    let account: Uuid = sqlx::query_scalar("INSERT INTO accounts (owner_id, kind, name, currency) VALUES ($1, 'cash', 'Wallet', 'PYG') RETURNING id")
        .bind(owner)
        .fetch_one(&pool)
        .await
        .expect("an account");
    let food: Uuid = sqlx::query_scalar("INSERT INTO categories (owner_id, flow, name) VALUES ($1, 'expense', 'food') RETURNING id")
        .bind(owner)
        .fetch_one(&pool)
        .await
        .expect("a category");
    let mut transaction = Actor::Person(owner).begin(&pool).await.expect("a transaction");
    let entry: Uuid = sqlx::query_scalar(
        "INSERT INTO entries (owner_id, occurred_on, cleared_on, description) VALUES ($1, '2026-10-01', '2026-10-01', 'lunch') RETURNING id",
    )
    .bind(owner)
    .fetch_one(&mut *transaction)
    .await
    .expect("an entry");
    sqlx::query(
        "INSERT INTO entry_lines (entry_id, side, account_id, category_id, amount, currency)
         VALUES ($1, 'account', $2, NULL, -45000, 'PYG'), ($1, 'category', NULL, $3, 45000, 'PYG')",
    )
    .bind(entry)
    .bind(account)
    .bind(food)
    .execute(&mut *transaction)
    .await
    .expect("the lines");
    transaction.commit().await.expect("recording");

    austeris_common::migrate::run(&pool, &MIGRATOR)
        .await
        .expect("migrating a schema that has entries");

    // The entry is the person's, as it was, and its history reads.
    let read = repository::entry(&pool, owner, entry).await.expect("reading").expect("the entry");
    assert!(!read.kept);
    assert!(repository::history(&pool, owner, entry).await.expect("reading a history").is_some());

    // With nothing kept, the schema rolls back without loss, and comes again.
    austeris_common::migrate::undo(&pool, &MIGRATOR, 20_261_020_100_001)
        .await
        .expect("rolling back books without holdings");
    austeris_common::migrate::run(&pool, &MIGRATOR).await.expect("migrating again");

    // A trade's money is something the previous schema cannot say, and it
    // refuses rather than drop it.
    let exchange = repository::create_account(&pool, owner, AccountKind::CryptoWallet, "Exchange", "USDT", Decimal::ZERO)
        .await
        .expect("an account in a stablecoin");
    grpc::Service::for_tests(pool.clone())
        .post_entry(tonic::Request::new(purchase(owner, exchange.id, true)))
        .await
        .expect("posting a purchase");
    let error = austeris_common::migrate::undo(&pool, &MIGRATOR, 20_261_020_100_001)
        .await
        .expect_err("a schema holding a trade's money rolled back");
    assert!(format!("{error:#}").contains("kept by a module"), "{error:#}");
}
