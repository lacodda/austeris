//! What an entry remembers, against a real PostgreSQL: who recorded and
//! changed it and what it was before, the files kept with it, and the trash a
//! deleted entry waits in.
//!
//! The history is written by the database, so it can only be shown there: that
//! a write from psql is recorded as one, that the service cannot write without
//! saying whose write it is, that one request is one revision however many rows
//! it touched, and that a deleted entry is gone from every balance while it
//! waits, then comes back under its own ids.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Actor, Config, USER_HEADER, db};
use austeris_ledger::model::{AccountKind, Flow};
use austeris_ledger::repository::{self, NewEntry, NewLine, Side, Swept};
use austeris_ledger::{MIGRATOR, grpc, routes};
use austeris_proto::ledger::v1::ledger_service_server::LedgerService as _;
use austeris_proto::ledger::v1::{Line as ProtoLine, PostEntryRequest, line::Side as ProtoSide};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use chrono::{DateTime, NaiveDate};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// The first bytes of a JPEG, and enough after them to be a file.
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01receipt";

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

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("a date")
}

/// Calls the service as the gateway would, with any body.
async fn send(pool: &PgPool, owner: Uuid, method: &str, uri: &str, content_type: &str, body: Vec<u8>) -> (StatusCode, HeaderMap, Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(USER_HEADER, owner.to_string())
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("a request");
    let response = routes::router(pool.clone()).oneshot(request).await.expect("an answer");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    (status, headers, bytes)
}

/// Calls the service with JSON, or nothing, and reads JSON back.
async fn call(pool: &PgPool, owner: Uuid, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let bytes = body.map(|value| value.to_string().into_bytes()).unwrap_or_default();
    let (status, _, answer) = send(pool, owner, method, uri, "application/json", bytes).await;
    let text = String::from_utf8(answer.to_vec()).expect("text");
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// Attaches a file to an entry.
async fn attach(pool: &PgPool, owner: Uuid, entry: &str, name: &str, bytes: &[u8]) -> (StatusCode, Value) {
    let uri = format!("/ledger/entries/{entry}/attachments?name={name}");
    let (status, _, answer) = send(pool, owner, "POST", &uri, "application/octet-stream", bytes.to_vec()).await;
    (status, serde_json::from_slice(&answer).unwrap_or(Value::Null))
}

/// A person with an account and a category to spend under.
struct Books {
    owner: Uuid,
    account: Uuid,
    food: Uuid,
    salary: Uuid,
}

async fn books(pool: &PgPool) -> Books {
    let owner = Uuid::new_v4();
    let account = repository::create_account(pool, owner, AccountKind::Cash, "Wallet", "PYG", decimal("100000"))
        .await
        .expect("an account");
    let food = repository::create_category(pool, owner, None, Flow::Expense, "food").await.expect("a category");
    let salary = repository::create_category(pool, owner, None, Flow::Income, "salary")
        .await
        .expect("a category");
    Books {
        owner,
        account: account.id,
        food: food.id,
        salary: salary.id,
    }
}

/// Records a spend through the API, as a person does, with whatever else the
/// body says.
async fn spend(pool: &PgPool, books: &Books, amount: &str, extra: Value) -> Value {
    let mut body = json!({
        "occurred_on": "2026-10-01",
        "description": "lunch",
        "lines": [
            { "account_id": books.account, "amount": format!("-{amount}"), "currency": "PYG" },
            { "category_id": books.food, "amount": amount, "currency": "PYG" },
        ],
    });
    if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        body.extend(extra.clone());
    }
    let (status, entry) = call(pool, books.owner, "POST", "/ledger/entries", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{entry}");
    entry
}

async fn history(pool: &PgPool, owner: Uuid, entry: &str) -> Vec<Value> {
    let (status, body) = call(pool, owner, "GET", &format!("/ledger/entries/{entry}/history"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().expect("a list").clone()
}

fn actions(history: &[Value]) -> Vec<&str> {
    history.iter().map(|revision| revision["action"].as_str().expect("an action")).collect()
}

async fn balance(pool: &PgPool, books: &Books) -> Decimal {
    repository::balances(pool, books.owner, day(31)).await.expect("balances")[0].amount
}

/// Runs a statement the way a repair script or psql would: from outside
/// austeris.
async fn outside(pool: &PgPool, sql: &'static str, bound: Uuid) {
    let mut transaction = pool.begin().await.expect("a transaction");
    sqlx::query("SELECT set_config('application_name', 'repair-script', true)")
        .execute(&mut *transaction)
        .await
        .expect("naming the connection");
    sqlx::query(sql).bind(bound).execute(&mut *transaction).await.expect("writing from outside");
    transaction.commit().await.expect("committing");
}

/// Moves what the trash holds back in time, as if it had been there for days:
/// from outside austeris, the way only a test would.
async fn age(pool: &PgPool, sql: &'static str, id: Uuid, days: i32) {
    let mut transaction = pool.begin().await.expect("a transaction");
    sqlx::query("SELECT set_config('application_name', 'test-clock', true)")
        .execute(&mut *transaction)
        .await
        .expect("naming the connection");
    sqlx::query(sql).bind(id).bind(days).execute(&mut *transaction).await.expect("ageing");
    transaction.commit().await.expect("committing");
}

#[tokio::test]
async fn an_entry_remembers_who_recorded_it_and_what_each_change_changed() {
    let Some(pool) = pool("test_memory_history").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({ "pending": true, "tags": ["work"] })).await;
    let id = entry["id"].as_str().expect("an id");

    let (status, body) = call(
        &pool,
        books.owner,
        "PATCH",
        &format!("/ledger/entries/{id}"),
        Some(json!({ "description": "dinner", "tags": ["work", "trip"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The hotel took more than it held: the header and every line are written
    // again, and the person did one thing.
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        &format!("/ledger/entries/{id}/clear"),
        Some(json!({ "on": "2026-10-03", "amount": "50000" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let revisions = history(&pool, books.owner, id).await;
    assert_eq!(actions(&revisions), ["created", "changed", "changed"], "{revisions:#?}");
    let me = json!({ "kind": "person", "person_id": books.owner });
    assert!(revisions.iter().all(|revision| revision["who"] == me), "{revisions:#?}");

    let recorded = &revisions[0];
    assert!(recorded["before"].is_null());
    assert_eq!(recorded["after"]["description"], "lunch");

    let renamed = &revisions[1];
    assert_eq!(renamed["changed"], json!(["description", "tags"]));
    assert_eq!(renamed["before"]["description"], "lunch");
    assert_eq!(renamed["before"]["tags"], json!(["work"]));
    assert_eq!(renamed["after"]["description"], "dinner");
    assert_eq!(renamed["after"]["tags"], json!(["trip", "work"]));

    let posted = &revisions[2];
    assert_eq!(posted["changed"], json!(["status", "lines"]));
    assert_eq!(posted["before"]["status"], "pending");
    assert_eq!(posted["after"]["cleared_on"], "2026-10-03");
    let account_amount = |state: &Value| {
        state["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .find(|line| line["side"] == "account")
            .map(|line| decimal(line["amount"].as_str().expect("an amount")))
            .expect("an account line")
    };
    assert_eq!(account_amount(&posted["before"]), decimal("-45000"));
    assert_eq!(account_amount(&posted["after"]), decimal("-50000"));

    // Deleted: the last thing it remembers is what it was.
    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let revisions = history(&pool, books.owner, id).await;
    let deleted = revisions.last().expect("a revision");
    assert_eq!(deleted["action"], "deleted");
    assert!(deleted["after"].is_null());
    assert_eq!(account_amount(&deleted["before"]), decimal("-50000"));

    // Nobody else reads it.
    let (status, _) = call(&pool, Uuid::new_v4(), "GET", &format!("/ledger/entries/{id}/history"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn saving_an_entry_as_it_was_is_not_a_change() {
    // The edit form sends every field back, and replacing the tags deletes and
    // writes them again: neither is something that happened to the entry.
    let Some(pool) = pool("test_memory_no_change").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({ "tags": ["work"] })).await;
    let id = entry["id"].as_str().expect("an id");

    let (status, body) = call(
        &pool,
        books.owner,
        "PATCH",
        &format!("/ledger/entries/{id}"),
        Some(json!({ "description": "lunch", "occurred_on": "2026-10-01", "tags": ["work"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(actions(&history(&pool, books.owner, id).await), ["created"]);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM entry_revisions WHERE entry_id = $1::uuid")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("counting");
    assert_eq!(stored, 1, "an empty revision was kept rather than dropped at commit");
}

#[tokio::test]
async fn a_module_and_a_script_are_each_named_for_what_they_did() {
    let Some(pool) = pool("test_memory_who").await else { return };
    let books = books(&pool).await;
    let service = grpc::Service::in_process(pool.clone());
    let posted = service
        .post_entry(tonic::Request::new(PostEntryRequest {
            owner_id: books.owner.to_string(),
            occurred_on: "2026-10-01".to_owned(),
            description: "October".to_owned(),
            idempotency_key: "salary-2026-10".to_owned(),
            source: "recurring".to_owned(),
            lines: vec![
                ProtoLine {
                    side: Some(ProtoSide::AccountId(books.account.to_string())),
                    amount: "2500000".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                ProtoLine {
                    side: Some(ProtoSide::CategoryId(books.salary.to_string())),
                    amount: "-2500000".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
            ..Default::default()
        }))
        .await
        .expect("posting")
        .into_inner();
    let id: Uuid = posted.entry_id.parse().expect("an id");

    outside(&pool, "UPDATE entries SET description = 'October salary' WHERE id = $1", id).await;

    let revisions = history(&pool, books.owner, &id.to_string()).await;
    assert_eq!(revisions[0]["who"], json!({ "kind": "module", "module": "recurring" }));
    assert_eq!(revisions[1]["who"], json!({ "kind": "outside" }));
    assert_eq!(revisions[1]["changed"], json!(["description"]));
}

#[tokio::test]
async fn austeris_changing_an_entry_without_saying_whose_change_it_is_is_refused() {
    // The guard on every path of the service that writes: one that forgot to
    // say on whose behalf it writes fails here, rather than leaving a history
    // that blames nobody.
    let Some(pool) = pool("test_memory_guard").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({})).await;

    let error = sqlx::query("UPDATE entries SET description = 'dinner' WHERE id = $1::uuid")
        .bind(entry["id"].as_str())
        .execute(&pool)
        .await
        .expect_err("the service wrote without naming anyone");
    assert!(error.to_string().contains("without saying on whose behalf"), "{error}");

    // Said, it is the person's.
    let mut transaction = Actor::Person(books.owner).begin(&pool).await.expect("a transaction");
    sqlx::query("UPDATE entries SET description = 'dinner' WHERE id = $1::uuid")
        .bind(entry["id"].as_str())
        .execute(&mut *transaction)
        .await
        .expect("writing as the person");
    transaction.commit().await.expect("committing");
    let revisions = history(&pool, books.owner, entry["id"].as_str().expect("an id")).await;
    assert_eq!(revisions[1]["who"]["kind"], "person");
}

#[tokio::test]
async fn a_deleted_tag_is_remembered_on_every_entry_it_was_on() {
    // By the time a tag's deletion cascades to the entries carrying it, the tag
    // is gone - the entries are remembered before that, with it on.
    let Some(pool) = pool("test_memory_tag").await else { return };
    let books = books(&pool).await;
    let first = spend(&pool, &books, "45000", json!({ "tags": ["trip"] })).await;
    let second = spend(&pool, &books, "30000", json!({ "tags": ["trip"], "description": "taxi" })).await;
    let tag = repository::tags(&pool, books.owner).await.expect("tags")[0].id;

    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/tags/{tag}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    for entry in [first, second] {
        let revisions = history(&pool, books.owner, entry["id"].as_str().expect("an id")).await;
        let untagged = revisions.last().expect("a revision");
        assert_eq!(untagged["changed"], json!(["tags"]), "{revisions:#?}");
        assert_eq!(untagged["before"]["tags"], json!(["trip"]));
        assert_eq!(untagged["after"]["tags"], json!([]));
    }
}

#[tokio::test]
async fn a_deleted_entry_waits_in_the_trash_and_comes_back_as_it_was() {
    let Some(pool) = pool("test_memory_trash").await else { return };
    let books = books(&pool).await;
    let entry = spend(
        &pool,
        &books,
        "45000",
        json!({ "tags": ["trip"], "place": { "country": "PY", "city": "Asunción" } }),
    )
    .await;
    let id = entry["id"].as_str().expect("an id").to_owned();
    let (status, file) = attach(&pool, books.owner, &id, "receipt.jpg", JPEG).await;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    let (_, kept) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(balance(&pool, &books).await, decimal("55000"));

    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Gone from the books: its money, its card, its list.
    assert_eq!(balance(&pool, &books).await, decimal("100000"), "a deleted entry still moved the balance");
    let (status, _) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, listed) = call(&pool, books.owner, "GET", "/ledger/entries", None).await;
    assert_eq!(listed, json!([]));

    // Waiting in the trash, with its file, until a date that says how long.
    let (status, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(status, StatusCode::OK, "{trash}");
    let trashed = &trash["entries"][0];
    assert_eq!(trashed["entry"]["id"], id.as_str());
    assert_eq!(trashed["entry"]["attachments"][0]["name"], "receipt.jpg");
    assert_eq!(trashed["who"]["kind"], "person");
    let at = |field: &str| DateTime::parse_from_rfc3339(trashed[field].as_str().expect("a time")).expect("a time");
    assert_eq!((at("erased_after") - at("deleted_at")).num_days(), 30);
    assert_eq!(trash["attachments"], json!([]), "a deleted entry's files are listed apart from it");
    // Its file can still be looked at, so it can be told apart from others.
    let (status, _, bytes) = send(
        &pool,
        books.owner,
        "GET",
        &format!("/ledger/attachments/{}/content", file["id"].as_str().expect("an id")),
        "",
        Vec::new(),
    )
    .await;
    assert_eq!((status, bytes.as_ref()), (StatusCode::OK, JPEG));
    // And it cannot be taken off an entry that is not in the books.
    let (status, _) = call(
        &pool,
        books.owner,
        "DELETE",
        &format!("/ledger/attachments/{}", file["id"].as_str().expect("an id")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Somebody else's restore finds nothing.
    let (status, _) = call(&pool, Uuid::new_v4(), "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, restored) = call(&pool, books.owner, "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    // As it was: the same id, the same lines under the same ids, the same
    // tags, place and file.
    assert_eq!(restored, kept);
    assert_eq!(balance(&pool, &books).await, decimal("55000"));
    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash, json!({ "keep_days": 30, "entries": [], "attachments": [] }));
    assert_eq!(
        actions(&history(&pool, books.owner, &id).await),
        ["created", "changed", "deleted", "restored"],
        "attached, deleted, restored"
    );

    // Restored once: a second press finds nothing in the trash.
    let (status, _) = call(&pool, books.owner, "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn two_restores_at_once_bring_one_entry_back() {
    let Some(pool) = pool("test_memory_restore_race").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({})).await;
    let id = entry["id"].as_str().expect("an id").to_owned();
    call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;

    let uri = format!("/ledger/trash/entries/{id}/restore");
    let presses: Vec<_> = (0..8)
        .map(|_| {
            let (pool, uri) = (pool.clone(), uri.clone());
            let owner = books.owner;
            tokio::spawn(async move { call(&pool, owner, "POST", &uri, None).await.0 })
        })
        .collect();
    let mut answers = Vec::new();
    for press in presses {
        answers.push(press.await.expect("a press"));
    }
    assert_eq!(answers.iter().filter(|status| **status == StatusCode::OK).count(), 1, "{answers:?}");
    assert!(
        answers.iter().all(|status| [StatusCode::OK, StatusCode::NOT_FOUND].contains(status)),
        "{answers:?}"
    );
    assert_eq!(balance(&pool, &books).await, decimal("55000"));
}

#[tokio::test]
async fn what_was_deleted_while_the_entry_waited_is_found_again_by_name() {
    let Some(pool) = pool("test_memory_restore_names").await else { return };
    let books = books(&pool).await;
    let shop = repository::create_counterparty(&pool, books.owner, "Casa Rica", None, None, None)
        .await
        .expect("a counterparty");
    let entry = spend(&pool, &books, "45000", json!({ "counterparty_id": shop.id, "tags": ["trip", "work"] })).await;
    let id = entry["id"].as_str().expect("an id").to_owned();
    call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;

    // With the entry in the trash nothing names the shop, so it can go; the
    // tag goes too, and the other is renamed.
    let (status, body) = call(&pool, books.owner, "DELETE", &format!("/ledger/counterparties/{}", shop.id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let tags = repository::tags(&pool, books.owner).await.expect("tags");
    let tag = |name: &str| tags.iter().find(|tag| tag.name == name).expect("a tag").id;
    call(&pool, books.owner, "DELETE", &format!("/ledger/tags/{}", tag("trip")), None).await;
    call(
        &pool,
        books.owner,
        "PATCH",
        &format!("/ledger/tags/{}", tag("work")),
        Some(json!({ "name": "office" })),
    )
    .await;

    let (status, restored) = call(&pool, books.owner, "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["counterparty"]["name"], "Casa Rica");
    assert_ne!(restored["counterparty"]["id"], json!(shop.id), "the deleted shop was not made again");
    // The renamed tag is the same tag; the deleted one is made again.
    assert_eq!(restored["tags"], json!(["office", "trip"]));
}

#[tokio::test]
async fn erasing_leaves_nothing_to_restore() {
    let Some(pool) = pool("test_memory_erase").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({})).await;
    let id = entry["id"].as_str().expect("an id").to_owned();
    let (_, file) = attach(&pool, books.owner, &id, "receipt.jpg", JPEG).await;
    call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;

    // Only what is in the trash is erased.
    let live = spend(&pool, &books, "1000", json!({})).await;
    let (status, _) = call(
        &pool,
        books.owner,
        "DELETE",
        &format!("/ledger/trash/entries/{}", live["id"].as_str().expect("an id")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/trash/entries/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(&pool, books.owner, "POST", &format!("/ledger/trash/entries/{id}/restore"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}/history"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the history outlived the erasing");
    let (status, _, _) = send(
        &pool,
        books.owner,
        "GET",
        &format!("/ledger/attachments/{}/content", file["id"].as_str().expect("an id")),
        "",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the file outlived the erasing");
}

#[tokio::test]
async fn emptying_the_trash_erases_all_of_it_and_nothing_else() {
    let Some(pool) = pool("test_memory_empty").await else { return };
    let books = books(&pool).await;
    let gone = spend(&pool, &books, "45000", json!({})).await;
    call(
        &pool,
        books.owner,
        "DELETE",
        &format!("/ledger/entries/{}", gone["id"].as_str().expect("an id")),
        None,
    )
    .await;
    let kept = spend(&pool, &books, "1000", json!({})).await;
    let kept_id = kept["id"].as_str().expect("an id").to_owned();
    let (_, removed) = attach(&pool, books.owner, &kept_id, "old.jpg", JPEG).await;
    call(
        &pool,
        books.owner,
        "DELETE",
        &format!("/ledger/attachments/{}", removed["id"].as_str().expect("an id")),
        None,
    )
    .await;
    let (_, on) = attach(&pool, books.owner, &kept_id, "new.jpg", b"\xFF\xD8\xFF\xE0another").await;

    let (status, _) = call(&pool, books.owner, "DELETE", "/ledger/trash", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash, json!({ "keep_days": 30, "entries": [], "attachments": [] }));
    let (_, entry) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{kept_id}"), None).await;
    assert_eq!(entry["attachments"][0]["id"], on["id"], "a file on an entry was erased with the trash");
}

#[tokio::test]
async fn the_trash_erases_what_it_has_kept_for_thirty_days() {
    let Some(pool) = pool("test_memory_sweep").await else { return };
    let books = books(&pool).await;
    let old = spend(&pool, &books, "45000", json!({})).await;
    let old_id: Uuid = old["id"].as_str().expect("an id").parse().expect("an id");
    let (_, old_file) = attach(&pool, books.owner, &old_id.to_string(), "old.jpg", JPEG).await;
    let fresh = spend(&pool, &books, "30000", json!({ "description": "taxi" })).await;
    for entry in [&old, &fresh] {
        call(
            &pool,
            books.owner,
            "DELETE",
            &format!("/ledger/entries/{}", entry["id"].as_str().expect("an id")),
            None,
        )
        .await;
    }
    age(
        &pool,
        "UPDATE entry_revisions SET at = at - make_interval(days => $2) WHERE entry_id = $1",
        old_id,
        31,
    )
    .await;

    // One entry deleted, restored and kept - its deletion is as old, and is not
    // its history's last word.
    let back = spend(&pool, &books, "1000", json!({ "description": "kept" })).await;
    let back_id: Uuid = back["id"].as_str().expect("an id").parse().expect("an id");
    let (_, on_back) = attach(&pool, books.owner, &back_id.to_string(), "kept.jpg", JPEG).await;
    call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{back_id}"), None).await;
    call(&pool, books.owner, "POST", &format!("/ledger/trash/entries/{back_id}/restore"), None).await;
    age(
        &pool,
        "UPDATE entry_revisions SET at = at - make_interval(days => $2) WHERE entry_id = $1",
        back_id,
        40,
    )
    .await;
    // And a file taken off it long ago, and one a moment ago.
    let (_, taken_long_ago) = attach(&pool, books.owner, &back_id.to_string(), "scan.jpg", b"\xFF\xD8\xFF\xE0scan").await;
    let (_, taken_now) = attach(&pool, books.owner, &back_id.to_string(), "photo.jpg", b"\xFF\xD8\xFF\xE0photo").await;
    for file in [&taken_long_ago, &taken_now] {
        call(
            &pool,
            books.owner,
            "DELETE",
            &format!("/ledger/attachments/{}", file["id"].as_str().expect("an id")),
            None,
        )
        .await;
    }
    let long_ago: Uuid = taken_long_ago["id"].as_str().expect("an id").parse().expect("an id");
    age(
        &pool,
        "UPDATE attachments SET removed_at = removed_at - make_interval(days => $2) WHERE id = $1",
        long_ago,
        31,
    )
    .await;

    // Not shown once it is past its time, whether or not it is erased yet.
    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash["entries"].as_array().map(Vec::len), Some(1), "{trash:#}");
    assert_eq!(trash["entries"][0]["entry"]["description"], "taxi");
    assert_eq!(trash["attachments"].as_array().map(Vec::len), Some(1), "{trash:#}");

    let swept = repository::sweep(&pool).await.expect("sweeping");
    assert_eq!(swept, Swept { entries: 1, attachments: 2 });
    assert_eq!(repository::sweep(&pool).await.expect("sweeping again"), Swept::default());

    let (status, _) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{old_id}/history"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let content = |id: &Value| format!("/ledger/attachments/{}/content", id.as_str().expect("an id"));
    let (status, _, _) = send(&pool, books.owner, "GET", &content(&old_file["id"]), "", Vec::new()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(&pool, books.owner, "GET", &content(&taken_long_ago["id"]), "", Vec::new()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // What was not past its time is all still there.
    for kept in [&on_back["id"], &taken_now["id"]] {
        let (status, _, _) = send(&pool, books.owner, "GET", &content(kept), "", Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{kept} was erased");
    }
    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash["entries"][0]["entry"]["description"], "taxi");
    assert_eq!(
        actions(&history(&pool, books.owner, &back_id.to_string()).await)[..4],
        ["created", "changed", "deleted", "restored"]
    );
}

#[tokio::test]
async fn a_file_is_kept_for_what_its_bytes_are_and_handed_back_safely() {
    let Some(pool) = pool("test_memory_files").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({})).await;
    let id = entry["id"].as_str().expect("an id").to_owned();

    // Named a PNG and sent as a page: kept as what it is.
    let uri = format!("/ledger/entries/{id}/attachments?name=%D1%87%D0%B5%D0%BA.png");
    let (status, _, body) = send(&pool, books.owner, "POST", &uri, "text/html", JPEG.to_vec()).await;
    assert_eq!(status, StatusCode::CREATED);
    let file: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(file["media_type"], "image/jpeg");
    assert_eq!(file["name"], "чек.png");
    assert_eq!(file["size"], JPEG.len());

    let (status, headers, bytes) = send(
        &pool,
        books.owner,
        "GET",
        &format!("/ledger/attachments/{}/content", file["id"].as_str().expect("an id")),
        "",
        Vec::new(),
    )
    .await;
    assert_eq!((status, bytes.as_ref()), (StatusCode::OK, JPEG));
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().expect("ascii").starts_with("inline; "));
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .expect("ascii")
            .ends_with("UTF-8''%D1%87%D0%B5%D0%BA.png")
    );

    // The entry says what is on it.
    let (_, read) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(read["attachments"][0]["id"], file["id"]);

    // An XML receipt is only ever saved, never opened as a page.
    let (status, xml) = attach(&pool, books.owner, &id, "factura.xml", b"<?xml version=\"1.0\"?><rDE/>").await;
    assert_eq!(status, StatusCode::CREATED, "{xml}");
    let (_, headers, _) = send(
        &pool,
        books.owner,
        "GET",
        &format!("/ledger/attachments/{}/content", xml["id"].as_str().expect("an id")),
        "",
        Vec::new(),
    )
    .await;
    assert!(headers[header::CONTENT_DISPOSITION].to_str().expect("ascii").starts_with("attachment; "));
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");

    // What it refuses, and how the screen is told.
    let (status, body) = attach(&pool, books.owner, &id, "receipt.jpg", b"<html><script>alert(1)</script></html>").await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::UNSUPPORTED_MEDIA_TYPE, Some("attachment.unsupported"))
    );
    let (status, body) = attach(&pool, books.owner, &id, "again.jpg", JPEG).await;
    assert_eq!((status, body["code"].as_str()), (StatusCode::CONFLICT, Some("attachment.already_attached")));
    assert_eq!(body["params"]["name"], "чек.png");
    let mut large = JPEG.to_vec();
    large.resize(austeris_ledger::attachment::MAX_BYTES + 1, 0);
    let (status, body) = attach(&pool, books.owner, &id, "large.jpg", &large).await;
    assert_eq!((status, body["code"].as_str()), (StatusCode::PAYLOAD_TOO_LARGE, Some("attachment.too_large")));
    assert_eq!(body["params"]["limit_mb"], 10);
    let (status, _) = attach(&pool, books.owner, &id, "empty.jpg", b"").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nobody else's: neither their entry to attach to, nor their file to read.
    let stranger = Uuid::new_v4();
    let (status, _) = attach(&pool, stranger, &id, "receipt.jpg", b"\xFF\xD8\xFF\xE0mine").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &pool,
        stranger,
        "GET",
        &format!("/ledger/attachments/{}/content", file["id"].as_str().expect("an id")),
        "",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_file_taken_off_an_entry_waits_in_the_trash() {
    let Some(pool) = pool("test_memory_file_trash").await else { return };
    let books = books(&pool).await;
    let entry = spend(&pool, &books, "45000", json!({})).await;
    let id = entry["id"].as_str().expect("an id").to_owned();
    let (_, file) = attach(&pool, books.owner, &id, "receipt.jpg", JPEG).await;
    let file_id = file["id"].as_str().expect("an id").to_owned();

    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/attachments/{file_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, read) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(read["attachments"], json!([]));
    let revisions = history(&pool, books.owner, &id).await;
    assert_eq!(actions(&revisions), ["created", "changed", "changed"]);
    assert_eq!(revisions[2]["changed"], json!(["attachments"]));
    assert_eq!(revisions[2]["before"]["attachments"][0]["name"], "receipt.jpg");

    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    let waiting = &trash["attachments"][0];
    assert_eq!(waiting["attachment"]["id"], file_id.as_str());
    assert_eq!(waiting["entry_id"], id.as_str());
    assert_eq!(waiting["entry_title"], "lunch");
    assert_eq!(waiting["entry_occurred_on"], "2026-10-01");

    // The same receipt attached again in the meantime: putting the old copy
    // back would make two.
    let (_, again) = attach(&pool, books.owner, &id, "receipt-again.jpg", JPEG).await;
    let (status, body) = call(&pool, books.owner, "POST", &format!("/ledger/trash/attachments/{file_id}/restore"), None).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("attachment.already_attached")),
        "{body}"
    );
    call(
        &pool,
        books.owner,
        "DELETE",
        &format!("/ledger/attachments/{}", again["id"].as_str().expect("an id")),
        None,
    )
    .await;

    let (status, back) = call(&pool, books.owner, "POST", &format!("/ledger/trash/attachments/{file_id}/restore"), None).await;
    assert_eq!(status, StatusCode::OK, "{back}");
    let (_, read) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert_eq!(read["attachments"][0]["id"], file_id.as_str());

    // A file of an entry in the trash goes back with the entry, not alone.
    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/attachments/{file_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{id}"), None).await;
    let (status, _) = call(&pool, books.owner, "POST", &format!("/ledger/trash/attachments/{file_id}/restore"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, trash) = call(&pool, books.owner, "GET", "/ledger/trash", None).await;
    assert_eq!(trash["attachments"], json!([]), "a file of a deleted entry is listed on its own");

    // Erasing a file taken off is for good, and only for such a file.
    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/trash/attachments/{file_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = send(&pool, books.owner, "GET", &format!("/ledger/attachments/{file_id}/content"), "", Vec::new()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A schema as v0.11 left it: a person's entry and a module's.
async fn books_from_before_history(pool: &PgPool) -> (Books, Uuid, Uuid) {
    austeris_common::migrate::undo(pool, &MIGRATOR, 20_261_012_100_001)
        .await
        .expect("rolling back to v0.11");
    let books = books(pool).await;
    let mut ids = Vec::new();
    for source in [None, Some("recurring")] {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO entries (owner_id, occurred_on, cleared_on, description, source, created_at)
             VALUES ($1, '2026-10-01', '2026-10-01', 'lunch', $2, '2026-10-01 12:00:00+00') RETURNING id",
        )
        .bind(books.owner)
        .bind(source)
        .fetch_one(pool)
        .await
        .expect("an entry");
        sqlx::query(
            "INSERT INTO entry_lines (entry_id, side, account_id, category_id, amount, currency)
             VALUES ($1, 'account', $2, NULL, -45000, 'PYG'), ($1, 'category', NULL, $3, 45000, 'PYG')",
        )
        .bind(id)
        .bind(books.account)
        .bind(books.food)
        .execute(pool)
        .await
        .expect("the lines");
        ids.push(id);
    }
    (books, ids[0], ids[1])
}

#[tokio::test]
async fn an_installation_with_entries_migrates_to_a_history() {
    let Some(pool) = pool("test_memory_migrate").await else { return };
    let (books, typed, posted) = books_from_before_history(&pool).await;

    austeris_common::migrate::run(&pool, &MIGRATOR)
        .await
        .expect("migrating a schema that has entries");

    // Each begins its history with being recorded, by whoever recorded it, on
    // the day it was.
    let typed_history = history(&pool, books.owner, &typed.to_string()).await;
    assert_eq!(actions(&typed_history), ["created"]);
    assert_eq!(typed_history[0]["who"], json!({ "kind": "person", "person_id": books.owner }));
    assert_eq!(typed_history[0]["at"], "2026-10-01T12:00:00Z");
    let posted_history = history(&pool, books.owner, &posted.to_string()).await;
    assert_eq!(posted_history[0]["who"], json!({ "kind": "module", "module": "recurring" }));

    // With nothing more than that, the schema rolls back without loss.
    austeris_common::migrate::undo(&pool, &MIGRATOR, 20_261_012_100_001)
        .await
        .expect("rolling back a history of recordings only");
    austeris_common::migrate::run(&pool, &MIGRATOR).await.expect("migrating again");

    // A change is something the previous schema would lose, and it refuses.
    repository::delete_entry(&pool, books.owner, &Actor::Person(books.owner), typed)
        .await
        .expect("deleting");
    let error = austeris_common::migrate::undo(&pool, &MIGRATOR, 20_261_012_100_001)
        .await
        .expect_err("a schema with an entry in the trash rolled back");
    assert!(format!("{error:#}").contains("in the trash"), "{error:#}");
}

#[tokio::test]
async fn a_post_through_the_repository_names_who_posted_it() {
    // What the demo and every module's contract go through.
    let Some(pool) = pool("test_memory_repository").await else { return };
    let books = books(&pool).await;
    let posted = repository::post_entry(
        &pool,
        books.owner,
        &Actor::module(Some("demo")),
        &NewEntry {
            occurred_on: day(1),
            description: "seeded".to_owned(),
            idempotency_key: Some("demo:2026-10-01:lunch".to_owned()),
            source: Some("demo".to_owned()),
            lines: vec![
                NewLine {
                    side: Side::Account(books.account),
                    amount: decimal("-1000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Category(books.food),
                    amount: decimal("1000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
            ..NewEntry::default()
        },
    )
    .await
    .expect("posting");
    let revisions = repository::history(&pool, books.owner, posted.entry.id)
        .await
        .expect("reading")
        .expect("a history");
    assert_eq!(revisions[0].who.module.as_deref(), Some("demo"));
    assert!(!repository::anything_remembered(&pool, books.owner).await.expect("asking"));
    repository::delete_entry(&pool, books.owner, &Actor::module(Some("demo")), posted.entry.id)
        .await
        .expect("deleting");
    assert!(repository::anything_remembered(&pool, books.owner).await.expect("asking"));
}
