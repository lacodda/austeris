//! What a module reading a document into the books can say through the ledger's
//! contract: who the money went to, by a tax number as well as a name, where it
//! happened, and the file it was read from - all in the one transaction that
//! writes the entry.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::time::Duration;

use austeris_common::{Config, db};
use austeris_ledger::model::{AccountKind, Flow};
use austeris_ledger::{MIGRATOR, grpc, repository};
use austeris_proto::ledger::v1::ledger_service_server::LedgerService as _;
use austeris_proto::ledger::v1::{
    CounterpartyName, File as ProtoFile, FindCounterpartyRequest, Line as ProtoLine, ListAccountsRequest, Place as ProtoPlace, PostEntryRequest, RateAtRequest,
    line::Side as ProtoSide,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
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

/// A person with a wallet and a groceries category.
struct Books {
    owner: Uuid,
    wallet: Uuid,
    groceries: Uuid,
}

async fn books(pool: &PgPool) -> Books {
    let owner = Uuid::new_v4();
    let wallet = repository::create_account(pool, owner, AccountKind::Cash, "Wallet", "PYG", Decimal::ZERO)
        .await
        .expect("creating an account");
    let groceries = repository::create_category(pool, owner, None, Flow::Expense, "Groceries")
        .await
        .expect("creating a category");
    Books {
        owner,
        wallet: wallet.id,
        groceries: groceries.id,
    }
}

/// A receipt of 45 000 guaranies as a module posts it, under its own key.
fn receipt(books: &Books, key: &str) -> PostEntryRequest {
    let line = |side: ProtoSide, amount: &str, note: &str| ProtoLine {
        side: Some(side),
        amount: amount.to_owned(),
        currency: "PYG".to_owned(),
        note: note.to_owned(),
    };
    PostEntryRequest {
        owner_id: books.owner.to_string(),
        occurred_on: "2026-10-18".to_owned(),
        description: String::new(),
        idempotency_key: key.to_owned(),
        source: "imports".to_owned(),
        lines: vec![
            line(ProtoSide::AccountId(books.wallet.to_string()), "-45000", ""),
            line(ProtoSide::CategoryId(books.groceries.to_string()), "30000", "LECHE ENTERA 1L"),
            line(ProtoSide::CategoryId(books.groceries.to_string()), "15000", "PAN FELIPE"),
        ],
        counterparty: Some(CounterpartyName {
            name: "Supermercado Sol".to_owned(),
            tax_id: "80012345-6".to_owned(),
            usual_category_id: books.groceries.to_string(),
        }),
        place: Some(ProtoPlace {
            country: "PY".to_owned(),
            city: "Asuncion".to_owned(),
        }),
        files: vec![ProtoFile {
            name: "factura.xml".to_owned(),
            content: b"<?xml version=\"1.0\"?><rDE/>".to_vec(),
        }],
        ..Default::default()
    }
}

async fn post(pool: &PgPool, request: PostEntryRequest) -> Result<(Uuid, bool), tonic::Status> {
    let response = grpc::Service::in_process(pool.clone())
        .post_entry(tonic::Request::new(request))
        .await?
        .into_inner();
    Ok((response.entry_id.parse().expect("an entry id"), response.created))
}

#[tokio::test]
async fn a_receipt_arrives_with_its_shop_its_place_and_its_file_in_one_entry() {
    let Some(pool) = pool("test_documents_whole").await else { return };
    let books = books(&pool).await;

    let (id, created) = post(&pool, receipt(&books, "sifen:1")).await.expect("posting the receipt");
    assert!(created);

    let entry = repository::entry(&pool, books.owner, id).await.expect("reading").expect("the entry");
    assert_eq!(entry.counterparty.as_ref().map(|c| c.name.as_str()), Some("Supermercado Sol"));
    let place = entry.place.expect("a place");
    assert_eq!((place.country.as_str(), place.city.as_deref()), ("PY", Some("Asuncion")));
    assert_eq!(entry.attachments.len(), 1, "the receipt is kept with the entry");
    assert_eq!(entry.attachments[0].media_type, "application/xml");
    assert_eq!(entry.lines.len(), 3);
    assert!(entry.lines.iter().any(|line| line.note == "PAN FELIPE" && line.amount == decimal("15000")));

    // The shop is new, and it is now known by its tax number and with the
    // category this receipt went on.
    let shop = repository::counterparty(&pool, books.owner, entry.counterparty.expect("a shop").id)
        .await
        .expect("reading")
        .expect("the shop");
    assert_eq!(shop.tax_id.as_deref(), Some("80012345-6"));
    assert_eq!(shop.default_category_id, Some(books.groceries));

    // The same receipt again is the entry made the first time - without a
    // second copy of the file.
    let (again, created) = post(&pool, receipt(&books, "sifen:1")).await.expect("posting it again");
    assert_eq!((again, created), (id, false));
    let entry = repository::entry(&pool, books.owner, id).await.expect("reading").expect("the entry");
    assert_eq!(entry.attachments.len(), 1);
}

#[tokio::test]
async fn a_receipt_finds_the_shop_by_its_tax_number_whatever_name_it_prints() {
    let Some(pool) = pool("test_documents_tax_id").await else { return };
    let books = books(&pool).await;
    post(&pool, receipt(&books, "sifen:1")).await.expect("the first receipt");

    // Printed under its legal name this time: still the same shop.
    let mut second = receipt(&books, "sifen:2");
    second.counterparty.as_mut().expect("a shop").name = "SOL S.A.".to_owned();
    let (id, _) = post(&pool, second).await.expect("the second receipt");
    let entry = repository::entry(&pool, books.owner, id).await.expect("reading").expect("the entry");
    assert_eq!(entry.counterparty.map(|c| c.name), Some("Supermercado Sol".to_owned()));
    assert_eq!(repository::counterparties(&pool, books.owner).await.expect("listing").len(), 1);

    let found = grpc::Service::in_process(pool.clone())
        .find_counterparty(tonic::Request::new(FindCounterpartyRequest {
            owner_id: books.owner.to_string(),
            tax_id: "80.012.345-6".to_owned(),
            name: "anything at all".to_owned(),
        }))
        .await
        .expect("finding")
        .into_inner()
        .counterparty
        .expect("found by its tax number");
    assert_eq!(found.name, "Supermercado Sol");
    assert_eq!(found.usual_category_id, books.groceries.to_string());
}

#[tokio::test]
async fn a_shop_typed_by_hand_gains_the_tax_number_but_keeps_its_own_category() {
    let Some(pool) = pool("test_documents_adopt").await else { return };
    let books = books(&pool).await;
    let eating_out = repository::create_category(&pool, books.owner, None, Flow::Expense, "Eating out")
        .await
        .expect("creating a category");
    let typed = repository::create_counterparty(&pool, books.owner, "Supermercado Sol", None, Some(eating_out.id), None)
        .await
        .expect("creating the shop by hand");

    post(&pool, receipt(&books, "sifen:1")).await.expect("posting the receipt");

    let shop = repository::counterparty(&pool, books.owner, typed.id)
        .await
        .expect("reading")
        .expect("the shop");
    assert_eq!(shop.tax_id.as_deref(), Some("80012345-6"), "the receipt's tax number was not adopted");
    assert_eq!(shop.default_category_id, Some(eating_out.id), "what the shop already had was replaced");
}

#[tokio::test]
async fn a_tax_number_another_shop_carries_is_not_moved_onto_this_one() {
    let Some(pool) = pool("test_documents_tax_taken").await else { return };
    let books = books(&pool).await;
    // The person gave the number to one shop; a receipt prints the name of
    // another they have.
    let holder = repository::create_counterparty(&pool, books.owner, "Sol Central", None, None, Some("80012345-6"))
        .await
        .expect("creating the holder");
    let named = repository::create_counterparty(&pool, books.owner, "Supermercado Sol", None, None, None)
        .await
        .expect("creating the one named");
    let (id, _) = post(&pool, receipt(&books, "sifen:1")).await.expect("posting the receipt");

    // The number wins, as when looking one up: the entry is the holder's, and
    // the one only named keeps having no number.
    let entry = repository::entry(&pool, books.owner, id).await.expect("reading").expect("the entry");
    assert_eq!(entry.counterparty.map(|c| c.id), Some(holder.id));
    let named = repository::counterparty(&pool, books.owner, named.id)
        .await
        .expect("reading")
        .expect("the shop");
    assert_eq!(named.tax_id, None);
    assert_eq!(repository::counterparties(&pool, books.owner).await.expect("listing").len(), 2);
}

#[tokio::test]
async fn a_city_is_the_same_city_however_its_accents_were_typed() {
    let Some(pool) = pool("test_documents_places").await else { return };
    let books = books(&pool).await;

    // The person's own place, typed with its accent.
    let mut typed = receipt(&books, "typed");
    typed.counterparty = None;
    typed.files.clear();
    typed.place.as_mut().expect("a place").city = "Asunción".to_owned();
    let (first, _) = post(&pool, typed).await.expect("the typed entry");

    // A till without accents prints it in capitals.
    let mut printed = receipt(&books, "sifen:1");
    printed.place.as_mut().expect("a place").city = "ASUNCION".to_owned();
    let (second, _) = post(&pool, printed).await.expect("the receipt");

    let place = |id| {
        let pool = pool.clone();
        let owner = books.owner;
        async move {
            repository::entry(&pool, owner, id)
                .await
                .expect("reading")
                .expect("the entry")
                .place
                .expect("a place")
        }
    };
    assert_eq!(place(first).await.id, place(second).await.id, "one city became two places");
    assert_eq!(place(second).await.city.as_deref(), Some("Asunción"));
    assert_eq!(repository::places(&pool, books.owner).await.expect("listing").len(), 1);
}

#[tokio::test]
async fn a_file_that_is_not_a_document_refuses_the_whole_entry() {
    let Some(pool) = pool("test_documents_bad_file").await else { return };
    let books = books(&pool).await;

    let mut request = receipt(&books, "sifen:1");
    request.files[0].content = b"<html><script>alert(1)</script>".to_vec();
    let refused = post(&pool, request).await.expect_err("a page was kept");
    assert_eq!(refused.code(), tonic::Code::InvalidArgument);

    // Nothing was written: no entry, and no shop either.
    let entries = repository::entries(
        &pool,
        books.owner,
        &repository::EntryFilter {
            limit: 10,
            ..Default::default()
        },
    )
    .await
    .expect("listing");
    assert_eq!(entries.len(), 0);
    assert_eq!(repository::counterparties(&pool, books.owner).await.expect("listing").len(), 0);
}

#[tokio::test]
async fn a_usual_category_of_another_persons_is_refused_as_the_callers_mistake() {
    let Some(pool) = pool("test_documents_not_theirs").await else { return };
    let books = books(&pool).await;
    let theirs = self::books(&pool).await;

    let mut request = receipt(&books, "sifen:1");
    request.counterparty.as_mut().expect("a shop").usual_category_id = theirs.groceries.to_string();
    let refused = post(&pool, request).await.expect_err("another person's category was taken");
    assert_eq!(refused.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn a_module_reads_the_accounts_and_the_rate_it_chooses_by() {
    let Some(pool) = pool("test_documents_reads").await else { return };
    let books = books(&pool).await;
    let card = repository::create_account(&pool, books.owner, AccountKind::Card, "Dollar card", "USD", Decimal::ZERO)
        .await
        .expect("creating a card");
    repository::set_account_closed(&pool, books.owner, card.id, true).await.expect("closing it");

    let service = grpc::Service::in_process(pool.clone());
    let accounts = service
        .list_accounts(tonic::Request::new(ListAccountsRequest {
            owner_id: books.owner.to_string(),
        }))
        .await
        .expect("listing")
        .into_inner()
        .accounts;
    assert_eq!(accounts.len(), 2);
    assert!(
        accounts
            .iter()
            .any(|a| a.name == "Wallet" && a.kind == "cash" && a.currency == "PYG" && !a.closed)
    );
    assert!(accounts.iter().any(|a| a.name == "Dollar card" && a.kind == "card" && a.closed));

    let day = NaiveDate::from_ymd_opt(2026, 10, 16).expect("a date");
    repository::record_rate(&pool, "USD", "PYG", day, decimal("7350"), "bcp")
        .await
        .expect("recording a rate");
    let asked = |on: &str| RateAtRequest {
        base: "pyg".to_owned(),
        quote: "USD".to_owned(),
        on: on.to_owned(),
    };
    let rate = service
        .rate_at(tonic::Request::new(asked("2026-10-18")))
        .await
        .expect("asking")
        .into_inner()
        .rate
        .expect("a rate, inverted");
    let value = Decimal::from_str(&rate.rate.as_ref().expect("the rate").rate).expect("a decimal");
    assert_eq!(value.round_dp(10), (Decimal::ONE / decimal("7350")).round_dp(10));
    assert_eq!(rate.rate.expect("the rate").on_date, "2026-10-16");
    assert!(!rate.stale);

    let before = service.rate_at(tonic::Request::new(asked("2026-10-01"))).await.expect("asking").into_inner();
    assert!(before.rate.is_none(), "a rate from after the day answered for it");
}
