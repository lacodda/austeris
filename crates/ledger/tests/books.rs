//! The ledger against a real PostgreSQL.
//!
//! What matters here lives in the database and cannot be shown any other way:
//! that an entry which does not balance is refused at commit rather than stored
//! and repaired, that a balance is the sum of its lines with nothing cached
//! beside it, that an idempotency key stops a retrying module from booking
//! twice, and that one person's books are not reachable from another's session.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Config, USER_HEADER, db};
// The gRPC surface is exercised through its own trait rather than over a
// socket: what is worth testing is the contract's answers, not tonic's
// transport.
use austeris_proto::ledger::v1::ledger_service_server::LedgerService as _;
use austeris_proto::ledger::v1::{GetBalancesRequest, Line as ProtoLine, PostEntryRequest, Rate as ProtoRate, RecordRatesRequest, line::Side as ProtoSide};

use austeris_ledger::model::{AccountKind, Flow, Purpose};
use austeris_ledger::repository::{NewEntry, NewLine, Side};
use austeris_ledger::{MIGRATOR, grpc, repository, routes};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::NaiveDate;
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

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("a date")
}

/// Calls the service as the gateway would, on behalf of a person.
async fn call(pool: &PgPool, owner: Uuid, method: &str, uri: &str, body: Option<&str>) -> (StatusCode, String) {
    let request = Request::builder().method(method).uri(uri).header(USER_HEADER, owner.to_string());
    let request = match body {
        Some(json) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(json.to_owned())),
        None => request.body(Body::empty()),
    }
    .unwrap();

    let response = routes::router(pool.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// A person with an account and the two categories these tests spend through.
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
        .expect("creating an account");
    let food = repository::create_category(pool, owner, None, Flow::Expense, "food")
        .await
        .expect("creating a category");
    let salary = repository::create_category(pool, owner, None, Flow::Income, "salary")
        .await
        .expect("creating a category");

    Books {
        owner,
        account: account.id,
        food: food.id,
        salary: salary.id,
    }
}

/// A two-sided entry: money leaving the account for a category.
fn spend(books: &Books, amount: &str, on: NaiveDate) -> NewEntry {
    NewEntry {
        occurred_on: on,
        description: "lunch".to_owned(),
        idempotency_key: None,
        source: None,
        lines: vec![
            NewLine {
                side: Side::Account(books.account),
                amount: -decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
            NewLine {
                side: Side::Category(books.food),
                amount: decimal(amount),
                currency: "PYG".to_owned(),
                note: String::new(),
            },
        ],
    }
}

#[tokio::test]
async fn an_entry_that_does_not_balance_is_refused_at_commit() {
    let Some(pool) = pool("test_ledger_balance").await else { return };
    let books = books(&pool).await;

    let mut lopsided = spend(&books, "45000", day(2026, 9, 17));
    // The money leaves the account but a different amount arrives at the
    // category: five hundred guaranies from nowhere.
    lopsided.lines[1].amount = decimal("44500");

    let error = repository::post_entry(&pool, books.owner, &lopsided)
        .await
        .expect_err("an unbalanced entry was stored");
    let message = format!("{error:#}");
    assert!(message.contains("does not balance"), "{message}");

    // And nothing survived the rollback: the header must not be left behind
    // with no lines, which is what a non-deferred constraint on the header
    // would have produced.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM entries WHERE owner_id = $1")
        .bind(books.owner)
        .fetch_one(&pool)
        .await
        .expect("counting entries");
    assert_eq!(count, 0, "a refused entry left a header behind");
}

#[tokio::test]
async fn a_two_sided_entry_commits_although_its_first_line_is_unbalanced() {
    // The point of the deferred constraint. Checked eagerly, the very first
    // INSERT of every entry would fail - there is no way to write the two sides
    // of a movement simultaneously.
    let Some(pool) = pool("test_ledger_deferred").await else { return };
    let books = books(&pool).await;

    let posted = repository::post_entry(&pool, books.owner, &spend(&books, "45000", day(2026, 9, 17)))
        .await
        .expect("a balanced entry was refused");

    assert_eq!(posted.entry.lines.len(), 2);
    assert!(posted.created);
    assert_eq!(posted.entry.total("PYG"), decimal("45000"));
}

#[tokio::test]
async fn a_balance_is_the_sum_of_the_lines_and_the_opening_amount() {
    let Some(pool) = pool("test_ledger_balances").await else { return };
    let books = books(&pool).await;

    repository::post_entry(&pool, books.owner, &spend(&books, "45000", day(2026, 9, 10)))
        .await
        .expect("posting");
    repository::post_entry(&pool, books.owner, &spend(&books, "5000", day(2026, 9, 12)))
        .await
        .expect("posting");

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading balances");
    let wallet = balances.iter().find(|b| b.account_id == books.account).expect("the account");
    // 100000 opening - 45000 - 5000.
    assert_eq!(wallet.amount, decimal("50000"));
}

#[tokio::test]
async fn a_balance_as_of_a_day_ignores_what_happened_after_it() {
    // "As of" is what makes a report of last March read the same next year.
    let Some(pool) = pool("test_ledger_as_of").await else { return };
    let books = books(&pool).await;

    repository::post_entry(&pool, books.owner, &spend(&books, "45000", day(2026, 9, 10)))
        .await
        .expect("posting");
    repository::post_entry(&pool, books.owner, &spend(&books, "30000", day(2026, 9, 20)))
        .await
        .expect("posting");

    let earlier = repository::balances(&pool, books.owner, day(2026, 9, 15)).await.expect("reading");
    let wallet = earlier.iter().find(|b| b.account_id == books.account).expect("the account");
    assert_eq!(wallet.amount, decimal("55000"), "a later entry leaked into an earlier balance");

    // The boundary itself is included: an entry on the day being asked about
    // has happened by the end of that day.
    let on_the_day = repository::balances(&pool, books.owner, day(2026, 9, 20)).await.expect("reading");
    let wallet = on_the_day.iter().find(|b| b.account_id == books.account).expect("the account");
    assert_eq!(wallet.amount, decimal("25000"), "an entry on the boundary day was excluded");
}

#[tokio::test]
async fn a_split_receipt_is_one_entry_with_one_payment() {
    let Some(pool) = pool("test_ledger_split").await else { return };
    let books = books(&pool).await;
    let transport = repository::create_category(&pool, books.owner, None, Flow::Expense, "transport")
        .await
        .expect("creating a category");

    let posted = repository::post_entry(
        &pool,
        books.owner,
        &NewEntry {
            occurred_on: day(2026, 9, 17),
            description: "supermarket".to_owned(),
            idempotency_key: None,
            source: None,
            lines: vec![
                NewLine {
                    side: Side::Account(books.account),
                    amount: decimal("-50000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Category(books.food),
                    amount: decimal("30000"),
                    currency: "PYG".to_owned(),
                    note: "groceries".to_owned(),
                },
                NewLine {
                    side: Side::Category(transport.id),
                    amount: decimal("20000"),
                    currency: "PYG".to_owned(),
                    note: "bus fare".to_owned(),
                },
            ],
        },
    )
    .await
    .expect("posting a split");

    assert_eq!(posted.entry.lines.len(), 3);
    // The total is the payment, not the sum of every line and not any one part.
    assert_eq!(posted.entry.total("PYG"), decimal("50000"));
}

#[tokio::test]
async fn a_transfer_between_accounts_touches_no_category() {
    // Money moving between two of a person's own accounts is not income and
    // not an expense; a ledger that needs a category for it reports spending
    // that never happened.
    let Some(pool) = pool("test_ledger_transfer").await else { return };
    let books = books(&pool).await;
    let bank = repository::create_account(&pool, books.owner, AccountKind::Bank, "Bank", "PYG", Decimal::ZERO)
        .await
        .expect("creating an account");

    repository::post_entry(
        &pool,
        books.owner,
        &NewEntry {
            occurred_on: day(2026, 9, 17),
            description: "to the bank".to_owned(),
            idempotency_key: None,
            source: None,
            lines: vec![
                NewLine {
                    side: Side::Account(books.account),
                    amount: decimal("-60000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Account(bank.id),
                    amount: decimal("60000"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
        },
    )
    .await
    .expect("posting a transfer");

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    let wallet = balances.iter().find(|b| b.account_id == books.account).expect("the wallet");
    let bank_balance = balances.iter().find(|b| b.account_id == bank.id).expect("the bank account");
    assert_eq!(wallet.amount, decimal("40000"));
    assert_eq!(bank_balance.amount, decimal("60000"));

    // And it shows up in no category, so no expense report counts it.
    let spent: Option<Decimal> = sqlx::query_scalar("SELECT SUM(amount) FROM entry_lines WHERE category_id IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("summing categories");
    assert!(spent.is_none() || spent == Some(Decimal::ZERO), "a transfer was recorded as spending");
}

#[tokio::test]
async fn an_exchange_moves_each_account_in_its_own_currency_and_files_what_was_kept() {
    let Some(pool) = pool("test_ledger_exchange").await else { return };
    let books = books(&pool).await;
    let dollars = repository::create_account(&pool, books.owner, AccountKind::Cash, "Dollars", "USD", Decimal::ZERO)
        .await
        .expect("creating an account");
    // Thursday's central bank rate; the exchange happens on Friday.
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 24), decimal("5900.28"), "bcp")
        .await
        .expect("recording a rate");

    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/exchanges",
        Some(&format!(
            r#"{{"from_account":"{}","given":"60000","to_account":"{}","got":"10","occurred_on":"2026-09-25"}}"#,
            books.account, dollars.id
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let recorded: serde_json::Value = serde_json::from_str(&body).expect("json");

    // Each account moved in its own currency - the guarani wallet by guaranies
    // only, which is what the old four-line shape got wrong.
    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    let wallet = balances.iter().find(|b| b.account_id == books.account).expect("the wallet");
    let usd = balances.iter().find(|b| b.account_id == dollars.id).expect("the dollars");
    assert_eq!(wallet.amount, decimal("40000"));
    assert_eq!(usd.amount, decimal("10"));

    // Ten dollars were worth 59003 guaranies that day; the other 997 are what
    // the exchange office kept, on a line of their own.
    let conversion = &recorded["conversion"];
    assert_eq!(conversion["fee"]["currency"], "PYG");
    assert_eq!(decimal(conversion["fee"]["amount"].as_str().expect("a fee")), decimal("997"));
    assert_eq!(conversion["deal"]["base"], "USD");
    assert_eq!(decimal(conversion["deal"]["rate"].as_str().expect("a rate")), decimal("6000"));
    assert_eq!(conversion["reference"]["source"], "bcp");
    assert_eq!(conversion["reference"]["stale"], false);

    let lines = recorded["entry"]["lines"].as_array().expect("lines");
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert_eq!(lines.iter().filter(|line| line["side"] == "conversion").count(), 2);

    // The fee went to the ledger's own category, created on first use.
    let fees = repository::purpose_category(&pool, books.owner, Purpose::ExchangeFees)
        .await
        .expect("the category");
    assert_eq!(fees.name, "Exchange fees");
    let kept: Decimal = sqlx::query_scalar("SELECT SUM(amount) FROM entry_lines WHERE category_id = $1")
        .bind(fees.id)
        .fetch_one(&pool)
        .await
        .expect("summing fees");
    assert_eq!(kept, decimal("997"));
}

#[tokio::test]
async fn a_line_in_a_currency_other_than_its_accounts_is_refused() {
    // The shape v0.7 wrote an exchange in: a dollar line on the guarani wallet.
    // The wallet's balance would then add dollars to guaranies.
    let Some(pool) = pool("test_ledger_line_currency").await else { return };
    let books = books(&pool).await;
    let dollars = repository::create_account(&pool, books.owner, AccountKind::Cash, "Dollars", "USD", Decimal::ZERO)
        .await
        .expect("creating an account");

    let line = |side, amount: &str, currency: &str| NewLine {
        side,
        amount: decimal(amount),
        currency: currency.to_owned(),
        note: String::new(),
    };
    let error = repository::post_entry(
        &pool,
        books.owner,
        &NewEntry {
            occurred_on: day(2026, 9, 17),
            description: "changed money".to_owned(),
            idempotency_key: None,
            source: None,
            lines: vec![
                line(Side::Account(books.account), "-75000", "PYG"),
                line(Side::Account(dollars.id), "75000", "PYG"),
                line(Side::Account(books.account), "-10", "USD"),
                line(Side::Account(dollars.id), "10", "USD"),
            ],
        },
    )
    .await
    .expect_err("a dollar line on a guarani account was stored");
    assert!(format!("{error:#}").contains("the account is in"), "{error:#}");

    // And through the API it is the caller's mistake, not a fault.
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries",
        Some(&format!(
            r#"{{"lines":[{{"account_id":"{}","amount":"-10","currency":"USD"}},{{"account_id":"{}","amount":"10","currency":"USD"}}]}}"#,
            books.account, dollars.id
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("reaches it through a conversion"), "{body}");
}

#[tokio::test]
async fn a_conversion_that_does_not_convert_is_refused() {
    // Balanced in PYG, and the money went nowhere anyone reports on.
    let Some(pool) = pool("test_ledger_idle_conversion").await else { return };
    let books = books(&pool).await;

    let error = repository::post_entry(
        &pool,
        books.owner,
        &NewEntry {
            occurred_on: day(2026, 9, 17),
            description: "vanished".to_owned(),
            idempotency_key: None,
            source: None,
            lines: vec![
                NewLine {
                    side: Side::Account(books.account),
                    amount: decimal("-100"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Conversion,
                    amount: decimal("100"),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
        },
    )
    .await
    .expect_err("a conversion into nothing was stored");
    assert!(format!("{error:#}").contains("does not convert"), "{error:#}");
}

#[tokio::test]
async fn an_accounts_currency_does_not_change_under_its_lines() {
    let Some(pool) = pool("test_ledger_currency_fixed").await else { return };
    let books = books(&pool).await;
    repository::post_entry(&pool, books.owner, &spend(&books, "1000", day(2026, 9, 17)))
        .await
        .expect("posting");

    let error = sqlx::query("UPDATE accounts SET currency = 'USD' WHERE id = $1")
        .bind(books.account)
        .execute(&pool)
        .await
        .expect_err("the currency changed under the account's lines");
    assert!(error.to_string().contains("cannot change"), "{error}");
}

#[tokio::test]
async fn an_amount_in_another_currency_is_converted_at_the_days_rate() {
    let Some(pool) = pool("test_ledger_foreign").await else { return };
    let books = books(&pool).await;
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 24), decimal("5900.28"), "bcp")
        .await
        .expect("recording a rate");

    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/quick",
        Some(r#"{"text":"10 usd food streaming","occurred_on":"2026-09-25"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let recorded: serde_json::Value = serde_json::from_str(&body).expect("json");

    // The category keeps what was charged; the wallet moved by what that was
    // in guaranies, rounded to whole guaranies.
    let lines = recorded["entry"]["lines"].as_array().expect("lines");
    let on = |side: &str, currency: &str| {
        lines
            .iter()
            .find(|line| line["side"] == side && line["currency"] == currency)
            .map(|line| decimal(line["amount"].as_str().expect("an amount")))
    };
    assert_eq!(on("category", "USD"), Some(decimal("10")));
    assert_eq!(on("account", "PYG"), Some(decimal("-59003")));
    assert_eq!(lines.len(), 4, "a conversion at the day's rate has no fee: {lines:?}");
    assert_eq!(recorded["conversion"]["reference"]["on_date"], "2026-09-24");
    assert!(recorded["conversion"].get("fee").is_none());
}

#[tokio::test]
async fn a_rate_stated_with_the_amount_is_the_one_charged_and_the_difference_a_fee() {
    let Some(pool) = pool("test_ledger_foreign_stated").await else { return };
    let books = books(&pool).await;
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 24), decimal("5900.28"), "bcp")
        .await
        .expect("recording a rate");

    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/quick",
        Some(r#"{"text":"10 usd food @5965 streaming","occurred_on":"2026-09-25"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let recorded: serde_json::Value = serde_json::from_str(&body).expect("json");

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("100000") - decimal("59650"));
    assert_eq!(decimal(recorded["conversion"]["fee"]["amount"].as_str().expect("a fee")), decimal("647"));
    assert_eq!(recorded["entry"]["description"], "streaming");
}

#[tokio::test]
async fn an_amount_in_a_currency_with_no_known_rate_asks_for_one() {
    let Some(pool) = pool("test_ledger_foreign_unknown").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"10 usd food"}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("@5965"), "the refusal does not say how to state a rate: {body}");

    // A rate on an amount already in the account's currency would be ignored
    // while the person believed it applied.
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 food @7"}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn the_rate_in_force_says_how_old_it_is() {
    let Some(pool) = pool("test_ledger_rate_age").await else { return };
    let books = books(&pool).await;
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 24), decimal("5900.28"), "bcp")
        .await
        .expect("recording a rate");

    let (status, body) = call(&pool, books.owner, "GET", "/ledger/rates/at?base=usd&quote=PYG&on=2026-09-28", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rate: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!((rate["age_days"].as_i64(), rate["stale"].as_bool()), (Some(4), Some(false)));

    let (_, body) = call(&pool, books.owner, "GET", "/ledger/rates/at?base=USD&quote=PYG&on=2026-10-05", None).await;
    let rate: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!((rate["age_days"].as_i64(), rate["stale"].as_bool()), (Some(11), Some(true)));
    // Still answered: it is the last thing known, and the flag is what keeps
    // it from being read as today's.
    assert_eq!(decimal(rate["rate"].as_str().expect("a rate")), decimal("5900.28"));

    let (status, _) = call(&pool, books.owner, "GET", "/ledger/rates/at?base=USD&quote=PYG&on=2026-09-01", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a rate from after the day was used for it");
}

#[tokio::test]
async fn a_total_names_the_currencies_it_converted_at_a_stale_rate() {
    let Some(pool) = pool("test_ledger_total_stale").await else { return };
    let books = books(&pool).await;
    repository::record_rate(&pool, "PYG", "USD", day(2026, 9, 1), decimal("0.00017"), "bcp")
        .await
        .expect("recording a rate");

    let (_, body) = call(&pool, books.owner, "GET", "/ledger/balances?as_of=2026-09-25&currency=USD", None).await;
    let answer: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(answer["total"]["stale"], serde_json::json!(["PYG"]));
    assert_eq!(answer["accounts"][0]["rate_used"]["on_date"], "2026-09-01");
    assert_eq!(answer["accounts"][0]["rate_used"]["stale"], true);
}

#[tokio::test]
async fn guaranies_are_priced_in_roubles_through_the_dollar() {
    // The two central banks each publish their own currency per dollar; the
    // pair between them comes from neither.
    let Some(pool) = pool("test_ledger_cross").await else { return };
    let books = books(&pool).await;
    let service = grpc::Service::for_tests(pool.clone());
    for (source, quote, rate) in [("bcp", "PYG", "5000"), ("cbr", "RUB", "85")] {
        service
            .record_rates(tonic::Request::new(RecordRatesRequest {
                source: source.to_owned(),
                rates: vec![ProtoRate {
                    base: "USD".to_owned(),
                    quote: quote.to_owned(),
                    on_date: "2026-09-24".to_owned(),
                    rate: rate.to_owned(),
                }],
            }))
            .await
            .expect("recording rates");
    }

    let (_, body) = call(&pool, books.owner, "GET", "/ledger/balances?as_of=2026-09-25&currency=RUB", None).await;
    let answer: serde_json::Value = serde_json::from_str(&body).expect("json");
    // 100000 PYG = 20 USD = 1700 RUB.
    assert_eq!(decimal(answer["total"]["amount"].as_str().expect("a total")), decimal("1700"));
    assert_eq!(answer["accounts"][0]["rate_used"]["source"], "bcp+cbr via USD");
}

#[tokio::test]
async fn rates_from_a_source_never_replace_what_is_already_recorded() {
    let Some(pool) = pool("test_ledger_record_rates").await else { return };
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 24), decimal("6000"), "manual")
        .await
        .expect("recording a rate");

    let service = grpc::Service::for_tests(pool.clone());
    let push = |on_date: &str, rate: &str| RecordRatesRequest {
        source: "bcp".to_owned(),
        rates: vec![ProtoRate {
            base: "USD".to_owned(),
            quote: "PYG".to_owned(),
            on_date: on_date.to_owned(),
            rate: rate.to_owned(),
        }],
    };

    // The day a person typed a rate for keeps theirs.
    let answer = service
        .record_rates(tonic::Request::new(push("2026-09-24", "5900.28")))
        .await
        .expect("recording");
    assert_eq!(answer.into_inner().recorded, 0);
    let kept = repository::rate_at(&pool, "USD", "PYG", day(2026, 9, 24))
        .await
        .expect("reading")
        .expect("a rate");
    assert_eq!((kept.rate, kept.source.as_str()), (decimal("6000"), "manual"));

    // A new day is recorded once; pushed again - revised - it is kept as it was.
    let answer = service.record_rates(tonic::Request::new(push("2026-09-25", "5910"))).await.expect("recording");
    assert_eq!(answer.into_inner().recorded, 1);
    let answer = service.record_rates(tonic::Request::new(push("2026-09-25", "5999"))).await.expect("recording");
    assert_eq!(answer.into_inner().recorded, 0);
    let kept = repository::rate_at(&pool, "USD", "PYG", day(2026, 9, 25))
        .await
        .expect("reading")
        .expect("a rate");
    assert_eq!(kept.rate, decimal("5910"));

    // A module cannot pass its rates off as a person's.
    let mut manual = push("2026-09-26", "5920");
    manual.source = "manual".to_owned();
    let status = service
        .record_rates(tonic::Request::new(manual))
        .await
        .expect_err("a module wrote a manual rate");
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn a_category_the_person_already_made_is_adopted_rather_than_doubled() {
    let Some(pool) = pool("test_ledger_purpose").await else { return };
    let books = books(&pool).await;
    let theirs = repository::create_category(&pool, books.owner, None, Flow::Expense, "exchange FEES")
        .await
        .expect("creating");

    let found = repository::purpose_category(&pool, books.owner, Purpose::ExchangeFees)
        .await
        .expect("the category");
    assert_eq!(found.id, theirs.id);
    // Found by purpose from now on, whatever it is called.
    sqlx::query("UPDATE categories SET name = 'Bank cuts' WHERE id = $1")
        .bind(theirs.id)
        .execute(&pool)
        .await
        .expect("renaming");
    let again = repository::purpose_category(&pool, books.owner, Purpose::ExchangeFees)
        .await
        .expect("the category");
    assert_eq!(again.id, theirs.id);
    assert_eq!(
        repository::categories(&pool, books.owner).await.expect("listing").len(),
        3,
        "a second fee category was made"
    );
}

#[tokio::test]
async fn a_schema_holding_conversions_refuses_to_roll_back_past_them() {
    let Some(pool) = pool("test_ledger_rollback").await else { return };
    let books = books(&pool).await;
    let dollars = repository::create_account(&pool, books.owner, AccountKind::Cash, "Dollars", "USD", Decimal::ZERO)
        .await
        .expect("creating an account");
    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/exchanges",
        Some(&format!(
            r#"{{"from_account":"{}","given":"59000","to_account":"{}","got":"10"}}"#,
            books.account, dollars.id
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // v0.7's schema cannot say what a conversion is; rolling back would drop
    // the exchange or fail halfway, and it is refused before either.
    let error = austeris_common::migrate::undo(&pool, &MIGRATOR, 20_260_917_100_001)
        .await
        .expect_err("a schema with conversions rolled back");
    assert!(format!("{error:#}").contains("delete them before rolling back"), "{error:#}");
}

#[tokio::test]
async fn posting_the_same_key_twice_books_once() {
    let Some(pool) = pool("test_ledger_idempotency").await else { return };
    let books = books(&pool).await;

    let mut salary = spend(&books, "2500000", day(2026, 9, 1));
    salary.idempotency_key = Some("salary-2026-09".to_owned());
    salary.source = Some("recurring".to_owned());

    let first = repository::post_entry(&pool, books.owner, &salary).await.expect("posting");
    let second = repository::post_entry(&pool, books.owner, &salary).await.expect("posting again");

    assert!(first.created, "the first post did not create the entry");
    assert!(!second.created, "the second post claimed to create a second entry");
    assert_eq!(first.entry.id, second.entry.id, "a retry made a second entry");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM entries WHERE owner_id = $1")
        .bind(books.owner)
        .fetch_one(&pool)
        .await
        .expect("counting");
    assert_eq!(count, 1, "a retry booked twice");
}

#[tokio::test]
async fn two_people_with_the_same_key_are_two_entries() {
    // The key is unique per person, not globally: two modules posting
    // "salary-2026-09" for two people are posting two salaries.
    let Some(pool) = pool("test_ledger_key_scope").await else { return };
    let mine = books(&pool).await;
    let theirs = books(&pool).await;

    let mut ours = spend(&mine, "1000", day(2026, 9, 1));
    ours.idempotency_key = Some("salary-2026-09".to_owned());
    let mut yours = spend(&theirs, "1000", day(2026, 9, 1));
    yours.idempotency_key = Some("salary-2026-09".to_owned());

    assert!(repository::post_entry(&pool, mine.owner, &ours).await.expect("posting").created);
    assert!(
        repository::post_entry(&pool, theirs.owner, &yours).await.expect("posting").created,
        "one person's idempotency key blocked another person's entry"
    );
}

#[tokio::test]
async fn an_entry_cannot_name_somebody_elses_account() {
    // The foreign key would accept this: it points at the table, not at the
    // owner. Without the check in `post_entry`, a stale id from another
    // person's books would post money into them.
    let Some(pool) = pool("test_ledger_ownership").await else { return };
    let mine = books(&pool).await;
    let theirs = books(&pool).await;

    let mut trespass = spend(&mine, "1000", day(2026, 9, 17));
    trespass.lines[0].side = Side::Account(theirs.account);

    let error = repository::post_entry(&pool, mine.owner, &trespass)
        .await
        .expect_err("an entry reached another person's account");
    assert!(format!("{error:#}").contains("does not exist"), "{error:#}");
}

#[tokio::test]
async fn one_persons_entries_are_not_readable_from_anothers_session() {
    let Some(pool) = pool("test_ledger_isolation").await else { return };
    let mine = books(&pool).await;
    let theirs = books(&pool).await;

    let posted = repository::post_entry(&pool, mine.owner, &spend(&mine, "45000", day(2026, 9, 17)))
        .await
        .expect("posting");

    // Reading it as its owner works.
    let (status, _) = call(&pool, mine.owner, "GET", &format!("/ledger/entries/{}", posted.entry.id), None).await;
    assert_eq!(status, StatusCode::OK);

    // Reading it as somebody else is a 404, not a 403: whether an entry exists
    // is itself none of their business.
    let (status, _) = call(&pool, theirs.owner, "GET", &format!("/ledger/entries/{}", posted.entry.id), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = call(&pool, theirs.owner, "GET", "/ledger/entries", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "[]", "another person's entries were listed");
}

#[tokio::test]
async fn without_the_gateways_header_nothing_answers() {
    let Some(pool) = pool("test_ledger_unsigned").await else { return };

    // No `x-austeris-user-id`: the request never reaches a handler, so there is
    // no "default person" whose books get read.
    let response = routes::router(pool.clone())
        .oneshot(Request::builder().uri("/ledger/accounts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_typed_line_becomes_an_entry_against_the_persons_own_account() {
    let Some(pool) = pool("test_ledger_quick").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/quick",
        Some(r#"{"text":"45000 food lunch at the corner","occurred_on":"2026-09-17"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let recorded: serde_json::Value = serde_json::from_str(&body).expect("json");
    let entry = &recorded["entry"];
    assert_eq!(entry["description"], "lunch at the corner");
    // A line in the account's own currency converts nothing, and says so by
    // saying nothing.
    assert!(recorded.get("conversion").is_none(), "{recorded}");
    assert_eq!(entry["lines"].as_array().expect("lines").len(), 2);

    // The account went down, the category went up.
    let lines = entry["lines"].as_array().expect("lines");
    let on_account = lines.iter().find(|line| !line["account_id"].is_null()).expect("an account line");
    let on_category = lines.iter().find(|line| !line["category_id"].is_null()).expect("a category line");
    // Compared as decimals, not as text: `NUMERIC(38, 18)` hands back the
    // scale it stores at, so the string is "-45000.000000000000000000". That
    // is the column being honest about its precision, the same way `market`
    // reports a price - what matters here is the number and its sign.
    assert_eq!(decimal(on_account["amount"].as_str().expect("a string")), decimal("-45000"));
    assert_eq!(decimal(on_category["amount"].as_str().expect("a string")), decimal("45000"));
    // And never a JSON number, which a browser would parse into a double.
    assert!(on_account["amount"].is_string(), "an amount reached a client as a number");

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("55000"));
}

#[tokio::test]
async fn a_typed_line_can_earn_as_well_as_spend() {
    let Some(pool) = pool("test_ledger_quick_income").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(
        &pool,
        books.owner,
        "POST",
        "/ledger/entries/quick",
        Some(r#"{"text":"+2500000 salary september"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let balances = repository::balances(&pool, books.owner, day(2030, 1, 1)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("2600000"), "income did not arrive in the account");
    let _ = books.salary;
}

#[tokio::test]
async fn a_typed_line_filed_under_the_wrong_kind_of_category_is_refused() {
    // `45000 salary` says money was spent on an income category. Recording it
    // would make every income report wrong, and nobody would see why.
    let Some(pool) = pool("test_ledger_quick_flow").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 salary"}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("money coming in"), "{body}");
}

#[tokio::test]
async fn a_typed_line_naming_an_unknown_category_says_so_rather_than_inventing_one() {
    let Some(pool) = pool("test_ledger_quick_unknown").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 yachts"}"#)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("yachts"), "{body}");
}

#[tokio::test]
async fn a_name_meaning_two_categories_is_asked_about_rather_than_guessed() {
    let Some(pool) = pool("test_ledger_ambiguous").await else { return };
    let books = books(&pool).await;

    let living = repository::create_category(&pool, books.owner, None, Flow::Expense, "living")
        .await
        .expect("creating");
    let travel = repository::create_category(&pool, books.owner, None, Flow::Expense, "travel")
        .await
        .expect("creating");
    repository::create_category(&pool, books.owner, Some(living.id), Flow::Expense, "taxi")
        .await
        .expect("creating");
    repository::create_category(&pool, books.owner, Some(travel.id), Flow::Expense, "taxi")
        .await
        .expect("creating");

    // Two leaves called "taxi". Picking one would file the money somewhere the
    // person did not say, and the report would be wrong in a way nobody traces.
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"25000 taxi"}"#)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // The path says which.
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"25000 travel/taxi"}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_typed_line_with_two_open_accounts_asks_which_rather_than_picking() {
    let Some(pool) = pool("test_ledger_which_account").await else { return };
    let books = books(&pool).await;
    repository::create_account(&pool, books.owner, AccountKind::Bank, "Bank", "PYG", Decimal::ZERO)
        .await
        .expect("creating");

    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 food"}"#)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("Wallet") && body.contains("Bank"), "{body}");

    // Naming it resolves the ambiguity.
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 food from bank"}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn the_rest_surface_refuses_an_entry_that_does_not_balance() {
    let Some(pool) = pool("test_ledger_rest_balance").await else { return };
    let books = books(&pool).await;

    let body = format!(
        r#"{{"occurred_on":"2026-09-17","description":"lunch","lines":[
            {{"account_id":"{}","amount":"-45000","currency":"PYG"}},
            {{"category_id":"{}","amount":"-44500","currency":"PYG"}}]}}"#,
        books.account, books.food
    );
    let (status, answer) = call(&pool, books.owner, "POST", "/ledger/entries", Some(&body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    // The answer names the currency and the gap, not a constraint's name.
    assert!(answer.contains("PYG") && answer.contains("89500"), "{answer}");
}

#[tokio::test]
async fn an_entry_is_deleted_whole_or_not_at_all() {
    let Some(pool) = pool("test_ledger_delete").await else { return };
    let books = books(&pool).await;

    let posted = repository::post_entry(&pool, books.owner, &spend(&books, "45000", day(2026, 9, 17)))
        .await
        .expect("posting");

    let (status, _) = call(&pool, books.owner, "DELETE", &format!("/ledger/entries/{}", posted.entry.id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Its lines went with it, rather than being left as money from nowhere.
    let lines: i64 = sqlx::query_scalar("SELECT count(*) FROM entry_lines WHERE entry_id = $1")
        .bind(posted.entry.id)
        .fetch_one(&pool)
        .await
        .expect("counting lines");
    assert_eq!(lines, 0, "the lines of a deleted entry survived it");

    // And the balance is back where it started.
    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("100000"));
}

#[tokio::test]
async fn an_account_with_entries_cannot_be_deleted_out_from_under_them() {
    let Some(pool) = pool("test_ledger_account_restrict").await else { return };
    let books = books(&pool).await;
    repository::post_entry(&pool, books.owner, &spend(&books, "45000", day(2026, 9, 17)))
        .await
        .expect("posting");

    // `ON DELETE RESTRICT`, not CASCADE: deleting an account must not silently
    // take a year of entries with it.
    let error = sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(books.account)
        .execute(&pool)
        .await
        .expect_err("an account with entries was deleted");
    assert!(error.as_database_error().is_some());
}

#[tokio::test]
async fn a_rate_is_the_newest_one_not_after_the_day_asked_about() {
    let Some(pool) = pool("test_ledger_rates").await else { return };

    repository::record_rate(&pool, "PYG", "USD", day(2026, 9, 11), decimal("0.00013"), "manual")
        .await
        .expect("recording");
    repository::record_rate(&pool, "PYG", "USD", day(2026, 9, 14), decimal("0.00014"), "manual")
        .await
        .expect("recording");

    // A Saturday uses Friday's rate.
    let friday = repository::rate_at(&pool, "PYG", "USD", day(2026, 9, 12))
        .await
        .expect("reading")
        .expect("a rate");
    assert_eq!(friday.rate, decimal("0.00013"));

    // A day before anything was recorded has no rate rather than the earliest
    // one: a rate from the future is not what was true then.
    assert!(
        repository::rate_at(&pool, "PYG", "USD", day(2026, 9, 1)).await.expect("reading").is_none(),
        "a later rate was used for an earlier day"
    );
}

#[tokio::test]
async fn a_rate_recorded_one_way_answers_the_other_way_round() {
    // PYG->USD and USD->PYG are the same fact said twice, and two rows that
    // must agree are two rows that can disagree.
    let Some(pool) = pool("test_ledger_rate_inverse").await else { return };
    repository::record_rate(&pool, "USD", "PYG", day(2026, 9, 17), decimal("7500"), "manual")
        .await
        .expect("recording");

    let inverse = repository::rate_at(&pool, "PYG", "USD", day(2026, 9, 17))
        .await
        .expect("reading")
        .expect("no inverted rate");
    // 1/7500 has no exact decimal form, so the reciprocal is right to the
    // digits a `Decimal` carries rather than exactly one. Twenty places is far
    // beyond any rate anyone quotes; asserting exact equality here would be
    // asserting something arithmetic cannot deliver.
    let round_trip = inverse.rate * decimal("7500");
    assert_eq!(
        round_trip.round_dp(20),
        Decimal::ONE,
        "the inverted rate is not the reciprocal: {}",
        inverse.rate
    );

    // A currency in itself is one, without a row saying so.
    let same = repository::rate_at(&pool, "USD", "USD", day(2026, 9, 17))
        .await
        .expect("reading")
        .expect("no identity rate");
    assert_eq!(same.rate, Decimal::ONE);
}

#[tokio::test]
async fn a_total_names_the_money_it_could_not_convert() {
    let Some(pool) = pool("test_ledger_total").await else { return };
    let books = books(&pool).await;
    // A second account in a currency with no rate recorded for it.
    repository::create_account(&pool, books.owner, AccountKind::CryptoWallet, "Wallet BTC", "BTC", decimal("0.5"))
        .await
        .expect("creating");
    repository::record_rate(&pool, "PYG", "USD", day(2026, 9, 17), decimal("0.00013"), "manual")
        .await
        .expect("recording");

    let (status, body) = call(&pool, books.owner, "GET", "/ledger/balances?as_of=2026-09-17&currency=USD", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let answer: serde_json::Value = serde_json::from_str(&body).expect("json");
    // 100000 PYG at 0.00013 is 13 USD; the bitcoin is not in the total. The
    // string carries the scale the multiplication produced, so it is compared
    // as a decimal rather than by its spelling.
    assert_eq!(decimal(answer["total"]["amount"].as_str().expect("a string")), decimal("13"));
    assert_eq!(answer["total"]["unconverted"], serde_json::json!(["BTC"]));

    // The balance that could not be converted carries no `converted` at all -
    // not a zero, which a reader could add up.
    let btc = answer["accounts"]
        .as_array()
        .expect("accounts")
        .iter()
        .find(|account| account["currency"] == "BTC")
        .expect("the bitcoin account");
    assert!(btc.get("converted").is_none(), "an unconvertible balance carried a converted amount: {btc}");
}

#[tokio::test]
async fn a_converted_amount_carries_the_ledgers_scale_not_the_products() {
    // Two eighteen-place decimals multiply into thirty-six places, which is an
    // artefact of the multiplication and not a fact about anyone's money. Seen
    // in a live run: a balance came back as "51.025000000000000000000000000".
    let Some(pool) = pool("test_ledger_scale").await else { return };
    let books = books(&pool).await;
    repository::record_rate(&pool, "PYG", "USD", day(2026, 9, 17), decimal("0.00013"), "manual")
        .await
        .expect("recording");

    let (status, body) = call(&pool, books.owner, "GET", "/ledger/balances?as_of=2026-09-17&currency=USD", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let answer: serde_json::Value = serde_json::from_str(&body).expect("json");
    let converted = answer["accounts"][0]["converted"].as_str().expect("a converted amount");
    let places = converted.split_once('.').map_or(0, |(_, tail)| tail.len());
    assert!(places <= 18, "a converted amount carried {places} decimal places: {converted}");
    // And it is still the right number: 100000 PYG at 0.00013.
    assert_eq!(decimal(converted), decimal("13"));
}

#[tokio::test]
async fn a_module_posts_through_the_contract_and_a_retry_books_once() {
    let Some(pool) = pool("test_ledger_grpc").await else { return };
    let books = books(&pool).await;
    let service = grpc::Service::for_tests(pool.clone());

    let request = || {
        tonic::Request::new(PostEntryRequest {
            owner_id: books.owner.to_string(),
            occurred_on: "2026-09-01".to_owned(),
            description: "September".to_owned(),
            idempotency_key: "salary-2026-09".to_owned(),
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
        })
    };

    let first = service.post_entry(request()).await.expect("posting").into_inner();
    assert!(first.created);

    let second = service.post_entry(request()).await.expect("posting again").into_inner();
    assert!(!second.created, "a retry claimed to create a second entry");
    assert_eq!(first.entry_id, second.entry_id);

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 30)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("2600000"), "a retry paid the salary twice");
}

#[tokio::test]
async fn a_module_posting_without_a_key_is_refused() {
    // A module retries; one with no key has no way to say that two attempts are
    // the same movement, and "at least once" delivery then means "sometimes
    // twice".
    let Some(pool) = pool("test_ledger_grpc_key").await else { return };
    let books = books(&pool).await;
    let service = grpc::Service::for_tests(pool.clone());

    let status = service
        .post_entry(tonic::Request::new(PostEntryRequest {
            owner_id: books.owner.to_string(),
            occurred_on: "2026-09-01".to_owned(),
            description: String::new(),
            idempotency_key: String::new(),
            source: "recurring".to_owned(),
            lines: vec![
                ProtoLine {
                    side: Some(ProtoSide::AccountId(books.account.to_string())),
                    amount: "1000".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                ProtoLine {
                    side: Some(ProtoSide::CategoryId(books.salary.to_string())),
                    amount: "-1000".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
        }))
        .await
        .expect_err("a keyless post was accepted");
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn a_module_told_its_own_arithmetic_is_wrong_rather_than_to_retry() {
    // An `internal` would have the module retrying forever over a mistake only
    // it can fix.
    let Some(pool) = pool("test_ledger_grpc_balance").await else { return };
    let books = books(&pool).await;
    let service = grpc::Service::for_tests(pool.clone());

    let status = service
        .post_entry(tonic::Request::new(PostEntryRequest {
            owner_id: books.owner.to_string(),
            occurred_on: "2026-09-01".to_owned(),
            description: String::new(),
            idempotency_key: "wrong-1".to_owned(),
            source: "recurring".to_owned(),
            lines: vec![
                ProtoLine {
                    side: Some(ProtoSide::AccountId(books.account.to_string())),
                    amount: "1000".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
                ProtoLine {
                    side: Some(ProtoSide::CategoryId(books.salary.to_string())),
                    amount: "-999".to_owned(),
                    currency: "PYG".to_owned(),
                    note: String::new(),
                },
            ],
        }))
        .await
        .expect_err("an unbalanced entry was accepted over the contract");
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
    assert!(status.message().contains("does not balance"), "{}", status.message());
}

#[tokio::test]
async fn balances_cross_the_contract_as_strings_with_every_digit() {
    let Some(pool) = pool("test_ledger_grpc_balances").await else { return };
    let books = books(&pool).await;
    // An amount that no double can hold: eighteen places is what a crypto
    // wallet actually needs (ADR 0004).
    repository::create_account(&pool, books.owner, AccountKind::CryptoWallet, "BTC", "BTC", decimal("0.123456789012345678"))
        .await
        .expect("creating");

    let service = grpc::Service::for_tests(pool.clone());
    let answer = service
        .get_balances(tonic::Request::new(GetBalancesRequest {
            owner_id: books.owner.to_string(),
            as_of: "2026-09-17".to_owned(),
            in_currency: String::new(),
        }))
        .await
        .expect("reading balances")
        .into_inner();

    let btc = answer.balances.iter().find(|balance| balance.currency == "BTC").expect("the wallet");
    assert_eq!(btc.amount, "0.123456789012345678", "a digit was lost crossing the contract");
    // Nothing was asked to be converted, so nothing claims to have been.
    assert!(btc.converted.is_empty());
}

#[tokio::test]
async fn an_amount_survives_storage_with_every_digit() {
    let Some(pool) = pool("test_ledger_precision").await else { return };
    let books = books(&pool).await;
    let wallet = repository::create_account(&pool, books.owner, AccountKind::CryptoWallet, "BTC", "BTC", Decimal::ZERO)
        .await
        .expect("creating");
    let fees = repository::create_category(&pool, books.owner, None, Flow::Expense, "fees")
        .await
        .expect("creating");

    let exact = "0.000000000000000001";
    repository::post_entry(
        &pool,
        books.owner,
        &NewEntry {
            occurred_on: day(2026, 9, 17),
            description: "a fee".to_owned(),
            idempotency_key: None,
            source: None,
            lines: vec![
                NewLine {
                    side: Side::Account(wallet.id),
                    amount: -decimal(exact),
                    currency: "BTC".to_owned(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Category(fees.id),
                    amount: decimal(exact),
                    currency: "BTC".to_owned(),
                    note: String::new(),
                },
            ],
        },
    )
    .await
    .expect("posting");

    let balances = repository::balances(&pool, books.owner, day(2026, 9, 17)).await.expect("reading");
    let btc = balances.iter().find(|balance| balance.account_id == wallet.id).expect("the wallet");
    assert_eq!(btc.amount.to_string(), format!("-{exact}"), "the smallest unit did not survive");
}

#[tokio::test]
async fn a_currency_code_that_is_also_one_of_the_persons_categories_is_the_category() {
    // `PEN` is the Peruvian sol, and `pen` is what this person files office
    // pens under. Read as a currency, the line would convert 45 000 soles.
    let Some(pool) = pool("test_ledger_currency_or_category").await else { return };
    let books = books(&pool).await;

    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 pen office"}"#)).await;
    // Without a category `pen`, it is a currency, and `office` is the category.
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.contains("`office`"), "{body}");

    repository::create_category(&pool, books.owner, None, Flow::Expense, "pen")
        .await
        .expect("creating");
    let (status, body) = call(&pool, books.owner, "POST", "/ledger/entries/quick", Some(r#"{"text":"45000 pen office"}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let recorded: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(recorded.get("conversion").is_none(), "{recorded}");
    assert_eq!(recorded["entry"]["description"], "office");
    let balances = repository::balances(&pool, books.owner, day(2030, 1, 1)).await.expect("reading");
    assert_eq!(balances[0].amount, decimal("55000"));
}
