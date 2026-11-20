//! Sorting the books out, against a real PostgreSQL: drafts recorded before
//! their category is known, one change made to many entries at once, and a
//! counterparty merged into another.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Actor, Config, USER_HEADER, db};
use austeris_ledger::model::{AccountKind, Flow, GroupBy};
use austeris_ledger::repository::{self, NewEntry, NewLine, Side, TotalsFilter};
use austeris_ledger::{MIGRATOR, routes};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::NaiveDate;
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
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

fn decimal(digits: &str) -> Decimal {
    Decimal::from_str(digits).expect("a decimal")
}

fn day(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).expect("a date")
}

/// Calls the service as the gateway would, on behalf of a person, and reads
/// JSON back.
async fn call(pool: &PgPool, owner: Uuid, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri).header(USER_HEADER, owner.to_string());
    let request = match body {
        Some(json) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(json.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("a request");
    let response = routes::router(pool.clone()).oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    let text = String::from_utf8(bytes.to_vec()).expect("text");
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// A person with two cash accounts in guaraníes, one in dollars, and the
/// categories these tests file under.
struct Books {
    owner: Uuid,
    wallet: Uuid,
    card: Uuid,
    dollars: Uuid,
    food: Uuid,
    home: Uuid,
}

async fn books(pool: &PgPool) -> Books {
    let owner = Uuid::new_v4();
    let account = |name: &'static str, currency: &'static str| async move {
        repository::create_account(pool, owner, AccountKind::Cash, name, currency, decimal("1000000"))
            .await
            .expect("creating an account")
            .id
    };
    let category = |name: &'static str| async move {
        repository::create_category(pool, owner, None, Flow::Expense, name)
            .await
            .expect("creating a category")
            .id
    };
    Books {
        owner,
        wallet: account("Wallet", "PYG").await,
        card: account("Card", "PYG").await,
        dollars: account("Dollars", "USD").await,
        food: category("food").await,
        home: category("home").await,
    }
}

/// Money leaving an account for a category, or for nothing said yet.
fn spend(account: Uuid, on: Side, amount: &str, day: NaiveDate) -> NewEntry {
    NewEntry {
        occurred_on: day,
        description: format!("spent {amount}"),
        lines: vec![
            NewLine {
                side: Side::Account(account),
                amount: -decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
            NewLine {
                side: on,
                amount: decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
        ],
        allow_duplicate: true,
        ..NewEntry::default()
    }
}

/// Money moved between two of the person's accounts.
fn transfer(from: Uuid, to: Uuid, amount: &str, day: NaiveDate) -> NewEntry {
    NewEntry {
        occurred_on: day,
        lines: vec![
            NewLine {
                side: Side::Account(from),
                amount: -decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
            NewLine {
                side: Side::Account(to),
                amount: decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
        ],
        allow_duplicate: true,
        ..NewEntry::default()
    }
}

async fn post(pool: &PgPool, books: &Books, entry: &NewEntry) -> Uuid {
    repository::post_entry(pool, books.owner, &Actor::Person(books.owner), entry)
        .await
        .expect("posting an entry")
        .entry
        .id
}

fn sides(entry: &Value) -> Vec<(String, Option<String>)> {
    entry["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|line| {
            (
                line["side"].as_str().expect("a side").to_owned(),
                line["category_id"].as_str().map(ToOwned::to_owned),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_draft_is_recorded_without_a_category_and_counted_until_it_is_sorted() {
    let Some(pool) = pool("test_ledger_drafts").await else { return };
    let books = books(&pool).await;

    // Recorded by hand: a line that says it is not sorted, and nothing else.
    let (status, draft) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries",
        Some(json!({
            "occurred_on": "2026-11-20",
            "lines": [
                { "account_id": books.wallet, "amount": "-45000", "currency": "PYG" },
                { "unsorted": true, "amount": "45000", "currency": "PYG" },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert!(sides(&draft).contains(&("unsorted".to_owned(), None)), "{draft}");
    // Sorted and not sorted at once is not a line.
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries",
        Some(json!({
            "lines": [
                { "account_id": books.wallet, "amount": "-1", "currency": "PYG" },
                { "unsorted": true, "category_id": books.food, "amount": "1", "currency": "PYG" },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "entry.line_side");

    post(&pool, &books, &spend(books.wallet, Side::Category(books.food), "20000", day(20))).await;
    let (_, count) = call(&pool, books.owner, "GET", "/ledger/entries/unsorted", None).await;
    assert_eq!(count["count"], 1, "{count}");
    let (_, drafts) = call(&pool, books.owner, "GET", "/ledger/entries?unsorted=true", None).await;
    assert_eq!(drafts.as_array().expect("entries").len(), 1, "{drafts}");
    assert_eq!(drafts[0]["id"], draft["id"]);
    let (_, sorted) = call(&pool, books.owner, "GET", "/ledger/entries?unsorted=false", None).await;
    assert_eq!(sorted.as_array().expect("entries").len(), 1, "{sorted}");

    // Its money is spending all the same, under the group of what is not
    // sorted yet: a month of drafts is not a month in which nothing was spent.
    let groups = repository::totals(&pool, books.owner, GroupBy::Category, &TotalsFilter::default())
        .await
        .expect("totals");
    let unsorted = groups.iter().find(|group| group.id.is_none()).expect("the unsorted group");
    assert_eq!(unsorted.amounts[0].spent, decimal("45000"));
    let food = groups.iter().find(|group| group.id == Some(books.food)).expect("food");
    assert_eq!(food.amounts[0].spent, decimal("20000"));

    // Sorted, it is a draft no longer.
    let id = draft["id"].as_str().expect("an id");
    let (status, done) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [id], "categorize": books.home })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["changed"], json!([id]));
    let (_, count) = call(&pool, books.owner, "GET", "/ledger/entries/unsorted", None).await;
    assert_eq!(count["count"], 0, "{count}");
    let (_, entry) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}"), None).await;
    assert!(sides(&entry).contains(&("category".to_owned(), Some(books.home.to_string()))), "{entry}");
    // And its history says who sorted it.
    let (_, history) = call(&pool, books.owner, "GET", &format!("/ledger/entries/{id}/history"), None).await;
    let last = history.as_array().expect("a history").last().expect("a revision").clone();
    assert_eq!(last["action"], "changed", "{history}");
    assert_eq!(last["who"]["person_id"], books.owner.to_string());
}

#[tokio::test]
async fn a_batch_files_drafts_and_the_persons_own_lines_and_leaves_the_rest() {
    let Some(pool) = pool("test_ledger_batch_categorize").await else { return };
    let books = books(&pool).await;
    let draft = post(&pool, &books, &spend(books.wallet, Side::Unsorted, "10000", day(18))).await;
    let filed = post(&pool, &books, &spend(books.wallet, Side::Category(books.food), "15000", day(18))).await;
    let transfer = post(&pool, &books, &transfer(books.wallet, books.card, "5000", day(18))).await;
    // An exchange's fee is filed under the ledger's own category, which a
    // batch leaves where it is.
    let fees = repository::purpose_category(&pool, books.owner, austeris_ledger::model::Purpose::ExchangeFees)
        .await
        .expect("the fee category");
    let fee = post(&pool, &books, &spend(books.wallet, Side::Category(fees.id), "700", day(18))).await;

    let (status, done) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [draft, filed, transfer, fee], "categorize": books.home })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let mut changed: Vec<String> = serde_json::from_value(done["changed"].clone()).expect("ids");
    changed.sort();
    let mut expected = vec![draft.to_string(), filed.to_string()];
    expected.sort();
    assert_eq!(changed, expected, "{done}");
    let mut unchanged: Vec<String> = serde_json::from_value(done["unchanged"].clone()).expect("ids");
    unchanged.sort();
    let mut expected = vec![transfer.to_string(), fee.to_string()];
    expected.sort();
    assert_eq!(unchanged, expected, "{done}");
    let fee_entry = repository::entry(&pool, books.owner, fee).await.expect("reading").expect("the fee");
    assert!(
        fee_entry.lines.iter().any(|line| line.category_id == Some(fees.id)),
        "the fee left its category"
    );

    // A category of somebody else's is refused, and nothing changes.
    let stranger = repository::create_category(&pool, Uuid::new_v4(), None, Flow::Expense, "theirs")
        .await
        .expect("another person's category");
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [draft], "categorize": stranger.id })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "category.not_found");
    // So is an entry of somebody else's, by how many.
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [draft, Uuid::new_v4()], "categorize": books.food })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!((body["code"].as_str(), body["params"]["count"].as_u64()), (Some("batch.missing"), Some(1)));
    let entry = repository::entry(&pool, books.owner, draft).await.expect("reading").expect("the draft");
    assert!(
        entry.lines.iter().any(|line| line.category_id == Some(books.home)),
        "a refused batch changed an entry"
    );

    // One change, said once.
    for body in [
        json!({ "ids": [draft] }),
        json!({ "ids": [draft], "categorize": books.food, "tag": { "add": ["x"] } }),
        json!({ "ids": [], "categorize": books.food }),
    ] {
        let (status, refused) = call(&pool, books.owner, "POST", "/ledger/entries/batch", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {refused}");
    }
}

#[tokio::test]
async fn a_batch_moves_money_to_another_account_of_the_same_currency() {
    let Some(pool) = pool("test_ledger_batch_move").await else { return };
    let books = books(&pool).await;
    let first = post(&pool, &books, &spend(books.wallet, Side::Category(books.food), "10000", day(18))).await;
    let second = post(&pool, &books, &spend(books.wallet, Side::Unsorted, "20000", day(19))).await;
    let elsewhere = post(&pool, &books, &spend(books.card, Side::Category(books.food), "30000", day(19))).await;

    let (status, done) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [first, second, elsewhere], "move": { "from": books.wallet, "to": books.card } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["unchanged"], json!([elsewhere.to_string()]), "{done}");
    let balances = repository::balances(&pool, books.owner, day(30)).await.expect("balances");
    let balance = |id: Uuid| balances.iter().find(|balance| balance.account_id == id).expect("a balance").amount;
    assert_eq!(balance(books.wallet), decimal("1000000"));
    assert_eq!(balance(books.card), decimal("940000"));

    let owner = books.owner;
    let refused = |body: Value| {
        let pool = pool.clone();
        async move { call(&pool, owner, "POST", "/ledger/entries/batch", Some(body)).await }
    };
    // Into another currency it would need a rate nobody stated.
    let (status, body) = refused(json!({ "ids": [first], "move": { "from": books.card, "to": books.dollars } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch.other_currency");
    // To where it is, it moves nothing.
    let (status, body) = refused(json!({ "ids": [first], "move": { "from": books.card, "to": books.card } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch.same_account");
    // A transfer between the two would become one from an account to itself.
    let transfer = post(&pool, &books, &transfer(books.wallet, books.card, "5000", day(20))).await;
    let (status, body) = refused(json!({ "ids": [transfer], "move": { "from": books.wallet, "to": books.card } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch.onto_itself");
    // A closed account takes nothing new.
    repository::set_account_closed(&pool, books.owner, books.wallet, true).await.expect("closing");
    let (status, body) = refused(json!({ "ids": [first], "move": { "from": books.card, "to": books.wallet } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch.closed_account");

    // A module's entry is moved by the module, and the batch says so.
    repository::set_account_closed(&pool, books.owner, books.wallet, false)
        .await
        .expect("reopening");
    let kept = repository::post_entry(
        &pool,
        books.owner,
        &Actor::Module(Some("portfolio".to_owned())),
        &NewEntry {
            occurred_on: day(20),
            description: "Buy 1 BTC".to_owned(),
            idempotency_key: Some("portfolio:trade:1".to_owned()),
            source: Some("portfolio".to_owned()),
            kept: true,
            lines: vec![
                NewLine {
                    side: Side::Account(books.card),
                    amount: decimal("-1000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Holding,
                    amount: decimal("1000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
            ..NewEntry::default()
        },
    )
    .await
    .expect("a module's entry")
    .entry
    .id;
    let (status, body) = refused(json!({ "ids": [first, kept], "move": { "from": books.card, "to": books.wallet } })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        (body["code"].as_str(), body["params"]["module"].as_str()),
        (Some("batch.kept"), Some("portfolio"))
    );
}

#[tokio::test]
async fn a_batch_puts_labels_on_and_takes_them_off() {
    let Some(pool) = pool("test_ledger_batch_tag").await else { return };
    let books = books(&pool).await;
    let mut labelled = spend(books.wallet, Side::Category(books.food), "10000", day(18));
    labelled.tags = vec!["work".to_owned()];
    let first = post(&pool, &books, &labelled).await;
    let second = post(&pool, &books, &spend(books.wallet, Side::Unsorted, "20000", day(19))).await;

    let (status, done) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [first, second], "tag": { "add": ["Trip-2026"], "remove": ["WORK"] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["unchanged"], json!([]), "{done}");
    for id in [first, second] {
        let entry = repository::entry(&pool, books.owner, id).await.expect("reading").expect("an entry");
        assert_eq!(entry.tags, vec!["Trip-2026".to_owned()], "{id}");
    }
    // Put on again, it changes nothing.
    let (_, again) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [first], "tag": { "add": ["trip-2026"] } })),
    )
    .await;
    assert_eq!(again["changed"], json!([]), "{again}");
    // A label is one word.
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [first], "tag": { "add": ["two words"] } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/batch", Some(json!({ "ids": [first], "tag": {} }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "batch.no_tags");
}

#[tokio::test]
async fn a_counterparty_merged_into_another_takes_its_entries_and_its_name_along() {
    let Some(pool) = pool("test_ledger_merge").await else { return };
    let books = books(&pool).await;
    let right = repository::create_counterparty(&pool, books.owner, "Superseis", None, None, None)
        .await
        .expect("a counterparty");
    let typo = repository::create_counterparty(&pool, books.owner, "Supersies", None, Some(books.food), Some("80012345-6"))
        .await
        .expect("a counterparty typed wrong");
    let mut entry = spend(books.wallet, Side::Category(books.food), "10000", day(18));
    entry.counterparty = Some(repository::CounterpartyChoice::Existing(typo.id));
    let moved = post(&pool, &books, &entry).await;

    let (status, merged) = call(
        &pool,
        books.owner,
        "POST",
        &format!("/ledger/counterparties/{}/merge", typo.id),
        Some(json!({ "into": right.id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["entries"], 1);
    // It takes what it lacked: the usual category and the tax number.
    assert_eq!(merged["counterparty"]["default_category_id"], books.food.to_string());
    assert_eq!(merged["counterparty"]["tax_id"], "80012345-6");
    let entry = repository::entry(&pool, books.owner, moved).await.expect("reading").expect("the entry");
    assert_eq!(entry.counterparty.map(|counterparty| counterparty.id), Some(right.id));
    let names: Vec<String> = repository::counterparties(&pool, books.owner)
        .await
        .expect("listing")
        .into_iter()
        .map(|counterparty| counterparty.name)
        .collect();
    assert_eq!(names, ["Superseis"]);

    // Typed the old way again, it finds the one it was merged into rather than
    // making the typo a third time.
    let (status, recorded) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/quick",
        Some(json!({ "text": "5000 @supersies", "account_id": books.wallet })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{recorded}");
    assert_eq!(recorded["entry"]["counterparty"]["id"], right.id.to_string(), "{recorded}");
    let still: Vec<String> = repository::counterparties(&pool, books.owner)
        .await
        .expect("listing")
        .into_iter()
        .map(|counterparty| counterparty.name)
        .collect();
    assert_eq!(still, ["Superseis"], "the typo came back");

    // A name given on purpose is the person's: a shop by that name is a new
    // shop, and the alias gives way to it.
    let (status, created) = call(&pool, books.owner, "POST", "/ledger/counterparties", Some(json!({ "name": "Supersies" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let found = repository::counterparty_by_name(&pool, books.owner, "supersies")
        .await
        .expect("looking up")
        .expect("found");
    assert_eq!(found.name, "Supersies");

    // Two businesses are not one typed two ways.
    let other = repository::create_counterparty(&pool, books.owner, "Stock", None, None, Some("80099999-1"))
        .await
        .expect("another business");
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        &format!("/ledger/counterparties/{}/merge", other.id),
        Some(json!({ "into": right.id })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "counterparty.merge_tax_ids");
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        &format!("/ledger/counterparties/{}/merge", right.id),
        Some(json!({ "into": right.id })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "counterparty.merge_itself");
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        &format!("/ledger/counterparties/{}/merge", Uuid::new_v4()),
        Some(json!({ "into": right.id })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn the_schema_rolls_back_over_drafts_only_once_they_are_sorted() {
    /// The version before drafts.
    const BEFORE_DRAFTS: i64 = 20_261_111_100_002;
    let Some(pool) = pool("test_ledger_drafts_migrate").await else { return };
    let books = books(&pool).await;
    let draft = post(&pool, &books, &spend(books.wallet, Side::Unsorted, "10000", day(18))).await;
    let error = austeris_common::migrate::undo(&pool, &MIGRATOR, BEFORE_DRAFTS)
        .await
        .expect_err("rolled back over a draft");
    assert!(format!("{error:#}").contains("drafts"), "{error:#}");

    // Sorted - and its history, which remembers it as a draft, erased with
    // it - the rollback goes through, and migrating again brings drafts back.
    let (status, done) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/batch",
        Some(json!({ "ids": [draft], "categorize": books.food })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    sqlx::query("DELETE FROM entry_revisions WHERE entry_id = $1")
        .bind(draft)
        .execute(&pool)
        .await
        .expect("forgetting the draft's history");
    austeris_common::migrate::undo(&pool, &MIGRATOR, BEFORE_DRAFTS)
        .await
        .expect("rolling back with no drafts left");
    austeris_common::migrate::run(&pool, &MIGRATOR).await.expect("migrating again");
    post(&pool, &books, &spend(books.wallet, Side::Unsorted, "1", day(19))).await;
}
