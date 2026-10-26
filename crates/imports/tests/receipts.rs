//! Receipts read into the books, against a real PostgreSQL and the ledger's own
//! contract in the same process.
//!
//! Two schemas per test, as an installation has: the ledger's, where the entry
//! lands, and this module's, where a document waits. The ledger is called
//! through its gRPC service without a socket - its real answers, its real
//! refusals.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use austeris_common::{Config, USER_HEADER, db};
use austeris_imports::book::Said;
use austeris_imports::inbox;
use austeris_imports::ledger::{self, Account, Books, Counterparty, People, Posted, Rate};
use austeris_imports::model::{Needed, Origin, State};
use austeris_imports::receive::{self, Received};
use austeris_imports::routes;
use austeris_ledger::model::{AccountKind, EntryStatus, Flow, LineSide};
use austeris_ledger::{grpc, repository};
use austeris_proto::ledger::v1::ledger_service_server::LedgerService as _;
use austeris_proto::ledger::v1::{FindCounterpartyRequest, ListAccountsRequest, PostEntryRequest, RateAtRequest};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::NaiveDate;
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::Status;
use tower::ServiceExt;
use uuid::Uuid;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// Opens a pool on a schema of this test's own, migrated from empty.
async fn pool(schema: &str, migrator: &'static sqlx::migrate::Migrator) -> Option<PgPool> {
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
    austeris_common::migrate::run(&pool, migrator).await.expect("migrating");
    Some(pool)
}

/// The ledger, called in this process through its own contract.
#[derive(Clone)]
struct Local(Arc<grpc::Service>);

impl Books for Local {
    async fn accounts(&self, owner: Uuid) -> Result<Vec<Account>, Status> {
        let listed = self
            .0
            .list_accounts(tonic::Request::new(ListAccountsRequest { owner_id: owner.to_string() }))
            .await?
            .into_inner();
        listed
            .accounts
            .into_iter()
            .map(|account| {
                Ok(Account {
                    id: ledger::id(&account.id)?,
                    name: account.name,
                    kind: account.kind,
                    currency: account.currency,
                    closed: account.closed,
                })
            })
            .collect()
    }

    async fn counterparty(&self, owner: Uuid, tax_id: Option<&str>, name: &str) -> Result<Option<Counterparty>, Status> {
        let found = self
            .0
            .find_counterparty(tonic::Request::new(FindCounterpartyRequest {
                owner_id: owner.to_string(),
                tax_id: tax_id.unwrap_or_default().to_owned(),
                name: name.to_owned(),
            }))
            .await?
            .into_inner();
        found.counterparty.map(ledger::counterparty).transpose()
    }

    async fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> Result<Option<Rate>, Status> {
        let found = self
            .0
            .rate_at(tonic::Request::new(RateAtRequest {
                base: base.to_owned(),
                quote: quote.to_owned(),
                on: on.to_string(),
            }))
            .await?
            .into_inner();
        found.rate.map(ledger::rate).transpose()
    }

    async fn post(&self, request: PostEntryRequest) -> Result<Posted, Status> {
        let posted = self.0.post_entry(tonic::Request::new(request)).await?.into_inner();
        Ok(Posted {
            entry_id: ledger::id(&posted.entry_id)?,
            created: posted.created,
        })
    }
}

/// A ledger nobody can reach.
#[derive(Clone)]
struct Down;

impl Books for Down {
    fn accounts(&self, _owner: Uuid) -> impl Future<Output = Result<Vec<Account>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn counterparty(&self, _owner: Uuid, _tax_id: Option<&str>, _name: &str) -> impl Future<Output = Result<Option<Counterparty>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn rate(&self, _base: &str, _quote: &str, _on: NaiveDate) -> impl Future<Output = Result<Option<Rate>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn post(&self, _request: PostEntryRequest) -> impl Future<Output = Result<Posted, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
}

/// One person, by address.
struct Person(&'static str, Uuid);

impl People for Person {
    fn person(&self, email: &str) -> impl Future<Output = Result<Option<Uuid>, Status>> {
        std::future::ready(Ok(email.eq_ignore_ascii_case(self.0).then_some(self.1)))
    }
}

/// A person's books in both schemas: a wallet in guaranies and a groceries
/// category.
struct Setup {
    ledger: PgPool,
    imports: PgPool,
    books: Local,
    owner: Uuid,
    wallet: Uuid,
    groceries: Uuid,
}

async fn setup(name: &str) -> Option<Setup> {
    let ledger = pool(&format!("test_imp_{name}_ledger"), &austeris_ledger::MIGRATOR).await?;
    let imports = pool(&format!("test_imp_{name}"), &austeris_imports::MIGRATOR).await?;
    let owner = Uuid::new_v4();
    let wallet = repository::create_account(&ledger, owner, AccountKind::Cash, "Wallet", "PYG", Decimal::ZERO)
        .await
        .expect("creating a wallet");
    let groceries = repository::create_category(&ledger, owner, None, Flow::Expense, "Groceries")
        .await
        .expect("creating a category");
    Some(Setup {
        books: Local(Arc::new(grpc::Service::in_process(ledger.clone()))),
        ledger,
        imports,
        owner,
        wallet: wallet.id,
        groceries: groceries.id,
    })
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)).expect("reading a fixture")
}

async fn receive(setup: &Setup, books: &impl Books, name: &str, content: &[u8], said: Said) -> Received {
    receive::receive(&setup.imports, books, setup.owner, name, content, Origin::Upload, said)
        .await
        .expect("receiving")
}

fn decimal(digits: &str) -> Decimal {
    Decimal::from_str(digits).expect("a decimal")
}

#[tokio::test]
async fn a_receipt_paid_in_cash_is_in_the_books_with_its_items_its_shop_and_its_file() {
    let Some(setup) = setup("cash").await else { return };
    // The shop is known, and so is what it usually is.
    repository::create_counterparty(&setup.ledger, setup.owner, "Supermercado Sol", None, Some(setup.groceries), None)
        .await
        .expect("creating the shop");

    let xml = fixture("sifen/invoice-cash.xml");
    let received = receive(&setup, &setup.books, "factura.xml", &xml, Said::default()).await;
    assert!(!received.already);
    let document = received.document;
    assert_eq!(document.state, State::Booked, "{document:?}");
    assert_eq!(document.shop.as_deref(), Some("Supermercado Sol"));
    let entry_id = document.entry_id.expect("an entry");

    let entry = repository::entry(&setup.ledger, setup.owner, entry_id)
        .await
        .expect("reading")
        .expect("the entry");
    assert_eq!(entry.occurred_on, NaiveDate::from_ymd_opt(2026, 10, 18).expect("a date"));
    assert_eq!(entry.status, EntryStatus::Cleared);
    assert_eq!(entry.source.as_deref(), Some("imports"));
    assert_eq!(entry.counterparty.as_ref().map(|c| c.name.as_str()), Some("Supermercado Sol"));
    let place = entry.place.clone().expect("a place");
    assert_eq!((place.country.as_str(), place.city.as_deref()), ("PY", Some("Asuncion")));
    assert_eq!(entry.attachments.len(), 1);
    assert_eq!(entry.attachments[0].name, "factura.xml");

    // The wallet moved by what was paid; each item is a line of its own.
    let wallet: Vec<Decimal> = entry
        .lines
        .iter()
        .filter(|line| line.side == LineSide::Account)
        .map(|line| line.amount)
        .collect();
    assert_eq!(wallet, vec![decimal("-32950")]);
    let items: Vec<&str> = entry
        .lines
        .iter()
        .filter(|line| line.side == LineSide::Category)
        .map(|line| line.note.as_str())
        .collect();
    assert_eq!(items.len(), 4, "{items:?}");
    assert!(
        entry
            .lines
            .iter()
            .all(|line| line.side != LineSide::Category || line.category_id == Some(setup.groceries))
    );

    // The shop now carries the tax number every later receipt finds it by.
    let shop = repository::counterparty(&setup.ledger, setup.owner, entry.counterparty.expect("a shop").id)
        .await
        .expect("reading")
        .expect("the shop");
    assert_eq!(shop.tax_id.as_deref(), Some("80099999-1"));

    // The file lives with the entry now, not here.
    let kept: Option<Option<Vec<u8>>> = sqlx::query_scalar("SELECT content FROM documents WHERE id = $1")
        .bind(document.id)
        .fetch_optional(&setup.imports)
        .await
        .expect("reading");
    assert_eq!(kept, Some(None));

    // The same file again is the same document and the same entry.
    let again = receive(&setup, &setup.books, "factura (1).xml", &xml, Said::default()).await;
    assert!(again.already);
    assert_eq!(again.document.id, document.id);
    assert_eq!(again.document.entry_id, Some(entry_id));

    // So is the same receipt as another file: the shop's system re-sent it,
    // indented differently.
    let reformatted = String::from_utf8(xml).expect("text").replace("\n    ", "\n");
    let resent = receive(&setup, &setup.books, "reenvio.xml", reformatted.as_bytes(), Said::default()).await;
    assert!(resent.already);
    assert_eq!(resent.document.entry_id, Some(entry_id));
    let entries = repository::entries(
        &setup.ledger,
        setup.owner,
        &repository::EntryFilter {
            limit: 10,
            ..Default::default()
        },
    )
    .await
    .expect("listing");
    assert_eq!(entries.len(), 1, "one receipt was booked twice");
}

#[tokio::test]
async fn a_receipt_that_does_not_say_what_it_needs_waits_until_the_person_does() {
    let Some(setup) = setup("waits").await else { return };
    let card = repository::create_account(&setup.ledger, setup.owner, AccountKind::Card, "Visa Debit", "PYG", Decimal::ZERO)
        .await
        .expect("creating a card");

    // Paid by card, at a pharmacy never seen before: which card, and what for,
    // are the person's to say.
    let waiting = receive(&setup, &setup.books, "farmacia.xml", &fixture("sifen/invoice-card.xml"), Said::default())
        .await
        .document;
    assert_eq!(waiting.state, State::Waiting);
    assert_eq!(waiting.needs, vec![Needed::Account, Needed::Category]);
    assert_eq!(waiting.receipt.as_ref().map(|receipt| receipt.total), Some(decimal("58000")));

    // Saying one keeps it while the other is still missing.
    let health = repository::create_category(&setup.ledger, setup.owner, None, Flow::Expense, "Health")
        .await
        .expect("creating a category");
    let half = receive::book_again(
        &setup.imports,
        &setup.books,
        setup.owner,
        waiting.id,
        Said {
            category: Some(health.id),
            ..Said::default()
        },
    )
    .await
    .expect("booking")
    .expect("the document");
    assert_eq!(half.needs, vec![Needed::Account]);
    assert_eq!(half.category_id, Some(health.id));

    let booked = receive::book_again(
        &setup.imports,
        &setup.books,
        setup.owner,
        waiting.id,
        Said {
            account: Some(card.id),
            ..Said::default()
        },
    )
    .await
    .expect("booking")
    .expect("the document");
    assert_eq!(booked.state, State::Booked, "{booked:?}");
    assert_eq!(booked.needs, Vec::new());

    // The pharmacy has learnt what it is: the next receipt from there needs
    // only the card.
    let next = String::from_utf8(fixture("sifen/invoice-card.xml"))
        .expect("text")
        .replace("01800888881001002000045622026101910000000027", "01800888881001002000045722026101910000000035");
    let second = receive(
        &setup,
        &setup.books,
        "farmacia-2.xml",
        next.as_bytes(),
        Said {
            account: Some(card.id),
            ..Said::default()
        },
    )
    .await
    .document;
    assert_eq!(second.state, State::Booked, "{second:?}");
    let entry = repository::entry(&setup.ledger, setup.owner, second.entry_id.expect("an entry"))
        .await
        .expect("reading")
        .expect("the entry");
    assert!(entry.lines.iter().any(|line| line.category_id == Some(health.id)));
}

#[tokio::test]
async fn a_receipt_from_another_currencys_account_is_held_at_the_days_rate_or_waits_for_one() {
    let Some(setup) = setup("currency").await else { return };
    let dollars = repository::create_account(&setup.ledger, setup.owner, AccountKind::Card, "Dollar card", "USD", Decimal::ZERO)
        .await
        .expect("creating a card");
    let said = Said {
        account: Some(dollars.id),
        category: Some(setup.groceries),
        ..Said::default()
    };

    // No rate known yet: it waits, and says for which pair and day.
    let waiting = receive(&setup, &setup.books, "farmacia.xml", &fixture("sifen/invoice-card.xml"), said.clone())
        .await
        .document;
    assert_eq!(waiting.state, State::Waiting);
    let reason = waiting.reason.expect("a reason");
    assert_eq!(reason.code, "document.no_rate");
    assert_eq!(reason.params["from"], "PYG");

    repository::record_rate(
        &setup.ledger,
        "USD",
        "PYG",
        NaiveDate::from_ymd_opt(2026, 10, 18).expect("a date"),
        decimal("7350"),
        "bcp",
    )
    .await
    .expect("recording a rate");
    let booked = receive::book_again(&setup.imports, &setup.books, setup.owner, waiting.id, Said::default())
        .await
        .expect("booking")
        .expect("the document");
    assert_eq!(booked.state, State::Booked, "{booked:?}");

    // Held, at what the day's rate made of it: the bank says what it took.
    let entry = repository::entry(&setup.ledger, setup.owner, booked.entry_id.expect("an entry"))
        .await
        .expect("reading")
        .expect("the entry");
    assert_eq!(entry.status, EntryStatus::Pending);
    let card: Vec<(Decimal, &str)> = entry
        .lines
        .iter()
        .filter(|line| line.side == LineSide::Account)
        .map(|line| (line.amount, line.currency.as_str()))
        .collect();
    assert_eq!(card, vec![(decimal("-7.89"), "USD")]);
}

#[tokio::test]
async fn what_is_not_a_receipt_is_refused_and_said_why() {
    let Some(setup) = setup("refused").await else { return };

    let other_xml = receive(
        &setup,
        &setup.books,
        "pedido.xml",
        b"<?xml version=\"1.0\"?><order><total>5</total></order>",
        Said::default(),
    )
    .await
    .document;
    assert_eq!(other_xml.state, State::Refused);
    assert_eq!(other_xml.reason.as_ref().map(|reason| reason.code.as_str()), Some("document.not_a_receipt"));

    let photo = receive(&setup, &setup.books, "ticket.jpg", b"\xFF\xD8\xFF\xE0\x00\x10JFIF", Said::default())
        .await
        .document;
    assert_eq!(photo.state, State::Refused);
    let reason = photo.reason.expect("a reason");
    assert_eq!(
        (reason.code.as_str(), &reason.params["media_type"]),
        ("document.unsupported", &serde_json::json!("image/jpeg"))
    );

    let delivery = String::from_utf8(fixture("sifen/invoice-cash.xml"))
        .expect("text")
        .replace("<iTiDE>1</iTiDE>", "<iTiDE>7</iTiDE>");
    let delivery = receive(&setup, &setup.books, "remision.xml", delivery.as_bytes(), Said::default())
        .await
        .document;
    assert_eq!(delivery.reason.map(|reason| reason.code), Some("document.not_a_payment".to_owned()));

    // Nothing reached the books, and no file is kept for any of them.
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM documents WHERE content IS NOT NULL")
        .fetch_one(&setup.imports)
        .await
        .expect("counting");
    assert_eq!(kept, 0);
}

#[tokio::test]
async fn a_receipt_that_arrives_while_the_books_are_down_waits_and_is_booked_when_asked_again() {
    let Some(setup) = setup("down").await else { return };
    let waiting = receive(&setup, &Down, "factura.xml", &fixture("sifen/invoice-cash.xml"), Said::default())
        .await
        .document;
    assert_eq!(waiting.state, State::Waiting);
    assert_eq!(waiting.reason.map(|reason| reason.code), Some("document.ledger_unavailable".to_owned()));

    // Nothing was lost: with the books back, the same document is booked.
    let booked = receive::book_again(
        &setup.imports,
        &setup.books,
        setup.owner,
        waiting.id,
        Said {
            category: Some(setup.groceries),
            ..Said::default()
        },
    )
    .await
    .expect("booking")
    .expect("the document");
    assert_eq!(booked.state, State::Booked, "{booked:?}");
    assert_eq!(booked.reason, None);
}

#[tokio::test]
async fn the_folder_takes_receipts_in_by_whose_and_which_account() {
    let Some(setup) = setup("folder").await else { return };
    let card = repository::create_account(&setup.ledger, setup.owner, AccountKind::Card, "Visa Debit", "PYG", Decimal::ZERO)
        .await
        .expect("creating a card");
    let health = repository::create_category(&setup.ledger, setup.owner, None, Flow::Expense, "Health")
        .await
        .expect("creating a category");
    repository::create_counterparty(&setup.ledger, setup.owner, "Farmacia Central", None, Some(health.id), None)
        .await
        .expect("creating the pharmacy");

    let root = std::env::temp_dir().join(format!("austeris-inbox-{}", Uuid::new_v4()));
    let mine = root.join("owner@example.test");
    std::fs::create_dir_all(mine.join("Visa Debit")).expect("the account's folder");
    std::fs::create_dir_all(root.join("stranger@example.test")).expect("a stranger's folder");
    std::fs::write(mine.join("Visa Debit").join("farmacia.xml"), fixture("sifen/invoice-card.xml")).expect("a receipt");
    std::fs::write(mine.join("notes.txt"), b"not a receipt").expect("a stray file");
    std::fs::write(mine.join("halfway.xml.part"), b"<?xml").expect("a file still arriving");
    std::fs::write(root.join("stranger@example.test").join("theirs.xml"), fixture("sifen/invoice-cash.xml")).expect("a stranger's receipt");

    let people = Person("owner@example.test", setup.owner);
    let mut warned = HashSet::new();
    let swept = inbox::sweep(&setup.imports, &setup.books, &people, &root, Duration::ZERO, &mut warned)
        .await
        .expect("sweeping");
    assert_eq!((swept.taken, swept.refused), (1, 1));

    // Paid from the account the folder is named after, filed as the pharmacy
    // usually is - with nothing said by hand.
    let documents = austeris_imports::repository::documents(
        &setup.imports,
        setup.owner,
        austeris_imports::repository::Filter {
            state: Some(State::Booked),
            limit: 10,
            offset: 0,
        },
    )
    .await
    .expect("listing");
    let booked = documents.into_iter().next().expect("a booked document").document();
    assert_eq!(booked.origin, Origin::Folder);
    let entry = repository::entry(&setup.ledger, setup.owner, booked.entry_id.expect("an entry"))
        .await
        .expect("reading")
        .expect("the entry");
    assert!(entry.lines.iter().any(|line| line.account_id == Some(card.id)));

    // Each file is moved out of the way of the next look.
    assert!(mine.join(inbox::DONE).join("farmacia.xml").is_file());
    assert!(mine.join(inbox::REFUSED).join("notes.txt").is_file());
    assert!(mine.join("halfway.xml.part").is_file(), "a file still arriving was taken");
    assert!(root.join("stranger@example.test").join("theirs.xml").is_file(), "nobody's folder was taken in");

    // A second look finds nothing new to do.
    let again = inbox::sweep(&setup.imports, &setup.books, &people, &root, Duration::ZERO, &mut warned)
        .await
        .expect("sweeping again");
    assert_eq!((again.taken, again.refused), (0, 0));

    // A folder named after an account the person does not have: the receipt
    // waits, saying so.
    std::fs::create_dir_all(mine.join("Mastercard")).expect("a folder");
    let other = String::from_utf8(fixture("sifen/invoice-card.xml"))
        .expect("text")
        .replace("01800888881001002000045622026101910000000027", "01800888881001002000046622026101910000000019");
    std::fs::write(mine.join("Mastercard").join("otra.xml"), other).expect("a receipt");
    inbox::sweep(&setup.imports, &setup.books, &people, &root, Duration::ZERO, &mut warned)
        .await
        .expect("sweeping");
    let waiting = austeris_imports::repository::documents(
        &setup.imports,
        setup.owner,
        austeris_imports::repository::Filter {
            state: Some(State::Waiting),
            limit: 10,
            offset: 0,
        },
    )
    .await
    .expect("listing");
    let waiting = waiting.into_iter().next().expect("a waiting document").document();
    assert_eq!(waiting.needs, vec![Needed::Account]);
    let reason = waiting.reason.expect("a reason");
    assert_eq!(
        (reason.code.as_str(), &reason.params["name"]),
        ("document.unknown_account", &serde_json::json!("Mastercard"))
    );

    // A file already said about - one that could not be moved out of the
    // folder - is left alone rather than read again every half minute.
    let stuck = mine.join("stuck.xml");
    std::fs::write(&stuck, fixture("sifen/credit-note.xml")).expect("a receipt");
    warned.insert(stuck.clone());
    let quiet = inbox::sweep(&setup.imports, &setup.books, &people, &root, Duration::ZERO, &mut warned)
        .await
        .expect("sweeping");
    assert_eq!((quiet.taken, quiet.refused), (0, 0));
    assert!(stuck.is_file());

    std::fs::remove_dir_all(&root).expect("cleaning up");
}

/// Calls the service as the gateway would.
async fn call(setup: &Setup, method: &str, uri: &str, body: Body, json: bool) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder().method(method).uri(uri).header(USER_HEADER, setup.owner.to_string());
    if json {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    let response = routes::router(setup.imports.clone(), setup.books.clone())
        .oneshot(request.body(body).expect("a request"))
        .await
        .expect("an answer");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

#[tokio::test]
async fn the_surface_takes_a_file_lists_what_waits_books_it_and_dismisses_it() {
    let Some(setup) = setup("surface").await else { return };
    let xml = fixture("sifen/invoice-card.xml");

    let (status, upload) = call(&setup, "POST", "/imports/documents?name=farmacia.xml", Body::from(xml.clone()), false).await;
    assert_eq!(status, StatusCode::CREATED, "{upload}");
    assert_eq!(upload["already"], false);
    assert_eq!(upload["document"]["state"], "waiting");
    assert_eq!(upload["document"]["needs"], serde_json::json!(["account", "category"]));
    let id = upload["document"]["id"].as_str().expect("an id").to_owned();

    let (status, again) = call(&setup, "POST", "/imports/documents?name=farmacia.xml", Body::from(xml), false).await;
    assert_eq!((status, &again["already"]), (StatusCode::OK, &serde_json::json!(true)));

    let (_, waiting) = call(&setup, "GET", "/imports/documents?state=waiting", Body::empty(), false).await;
    assert_eq!(waiting.as_array().map(Vec::len), Some(1));

    let body = serde_json::json!({ "account_id": setup.wallet, "category_id": setup.groceries }).to_string();
    let (status, booked) = call(&setup, "POST", &format!("/imports/documents/{id}/book"), Body::from(body), true).await;
    assert_eq!(status, StatusCode::OK, "{booked}");
    assert_eq!(booked["state"], "booked");
    assert!(booked["entry_id"].is_string());

    let (status, _) = call(&setup, "DELETE", &format!("/imports/documents/{id}"), Body::empty(), false).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&setup, "GET", &format!("/imports/documents/{id}"), Body::empty(), false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The entry it became stays in the books.
    let entry = repository::entry(&setup.ledger, setup.owner, booked["entry_id"].as_str().expect("an id").parse().expect("a uuid"))
        .await
        .expect("reading");
    assert!(entry.is_some());

    // Another person's document is nobody's to this one.
    let (status, _) = call(&setup, "POST", &format!("/imports/documents/{}/book", Uuid::new_v4()), Body::from("{}"), true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A slip has to be a picture.
    let (status, refused) = call(&setup, "POST", "/imports/slips", Body::from("<?xml version=\"1.0\"?><r/>"), false).await;
    assert_eq!(
        (status, &refused["code"]),
        (StatusCode::UNSUPPORTED_MEDIA_TYPE, &serde_json::json!("slip.unsupported"))
    );
}
