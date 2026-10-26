//! Trades, their money, what they add up to and what it is worth, against a
//! real PostgreSQL and the ledger's and the market's own contracts in the
//! same process.
//!
//! Three schemas per test, as an installation has: the ledger's, where a
//! trade's money lands; the market's, where instruments and prices are; and
//! this module's, where the trades are. The two services are called through
//! their gRPC implementations without a socket - their real answers, their
//! real refusals.
//!
//! Without `AUSTERIS_DATABASE_URL` these skip themselves, and the CI job that
//! owns them fails when they do.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use austeris_common::{Actor, Config, USER_HEADER, db};
use austeris_ledger::model::AccountKind;
use austeris_market::model::Kind;
use austeris_portfolio::services::{Account, Books, Cash, Local, Rate};
use austeris_portfolio::{MIGRATOR, routes};
use austeris_proto::ledger::v1::{PostEntryRequest, RestateEntryRequest};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
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
    austeris_common::migrate::run(&pool, migrator).await.expect("migrating");
    Some(pool)
}

fn decimal(digits: &str) -> Decimal {
    Decimal::from_str(digits).expect("a decimal")
}

/// The decimal a JSON field carries as a string.
fn number(value: &Value) -> Decimal {
    decimal(value.as_str().unwrap_or_else(|| panic!("{value} is not a decimal string")))
}

type Ledger = Local<austeris_ledger::grpc::Service>;
type Market = Local<austeris_market::grpc::Service>;

/// An installation: a person with an exchange account kept in USDT, and
/// Bitcoin in the market's catalogue.
struct World {
    ledger_pool: PgPool,
    market_pool: PgPool,
    pool: PgPool,
    owner: Uuid,
    exchange: Uuid,
    btc: Uuid,
}

impl World {
    async fn new(name: &str) -> Option<Self> {
        let ledger_pool = pool(&format!("test_{name}_ledger"), &austeris_ledger::MIGRATOR).await?;
        let market_pool = pool(&format!("test_{name}_market"), &austeris_market::MIGRATOR).await?;
        let pool = pool(&format!("test_{name}_portfolio"), &MIGRATOR).await?;
        let owner = Uuid::new_v4();
        let exchange = austeris_ledger::repository::create_account(&ledger_pool, owner, AccountKind::CryptoWallet, "Exchange", "USDT", decimal("1000"))
            .await
            .expect("an exchange account")
            .id;
        let btc = austeris_market::repository::upsert_instrument(&market_pool, Kind::Crypto, "BTC", "Bitcoin", Some(8))
            .await
            .expect("an instrument")
            .id;
        Some(Self {
            ledger_pool,
            market_pool,
            pool,
            owner,
            exchange,
            btc,
        })
    }

    fn books(&self) -> Ledger {
        Local(Arc::new(austeris_ledger::grpc::Service::in_process(self.ledger_pool.clone())))
    }

    fn market(&self) -> Market {
        Local(Arc::new(austeris_market::grpc::Service::in_process(self.market_pool.clone())))
    }

    fn app(&self) -> Router {
        routes::router(self.pool.clone(), self.books(), self.market())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        call(self.app(), self.owner, method, uri, body).await
    }

    async fn account(&self, kind: AccountKind, name: &str, currency: &str) -> Uuid {
        austeris_ledger::repository::create_account(&self.ledger_pool, self.owner, kind, name, currency, Decimal::ZERO)
            .await
            .expect("an account")
            .id
    }

    async fn instrument(&self, kind: Kind, symbol: &str) -> Uuid {
        austeris_market::repository::upsert_instrument(&self.market_pool, kind, symbol, symbol, None)
            .await
            .expect("an instrument")
            .id
    }

    async fn price(&self, instrument: Uuid, currency: &str, at: DateTime<Utc>, price: &str) {
        austeris_market::repository::record_price(&self.market_pool, instrument, currency, at, decimal(price), "test")
            .await
            .expect("a price");
    }

    /// What the exchange account holds, as the ledger adds it up.
    async fn cash(&self) -> Decimal {
        let today = Utc::now().date_naive();
        let balances = austeris_ledger::repository::balances(&self.ledger_pool, self.owner, today)
            .await
            .expect("balances");
        balances
            .iter()
            .find(|balance| balance.account_id == self.exchange)
            .expect("the exchange")
            .amount
    }

    /// Records a trade and expects it recorded.
    async fn trade(&self, body: Value) -> Value {
        let (status, trade) = self.call("POST", "/portfolio/trades", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{trade}");
        trade
    }
}

/// Calls a router as the gateway would, and reads JSON back.
async fn call(app: Router, owner: Uuid, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(USER_HEADER, owner.to_string())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
        .expect("a request");
    let response = app.oneshot(request).await.expect("an answer");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("a body").to_bytes();
    let text = String::from_utf8(bytes.to_vec()).expect("text");
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// An hour from the start of 20 October 2026 in Asunción.
fn hour(hours: u32) -> String {
    format!("2026-10-{:02}T{:02}:00:00-03:00", 20 + hours / 24, hours % 24)
}

/// A buy, at an hour from the start of 20 October 2026 in Asunción.
fn buy(account: Uuid, instrument: Uuid, quantity: &str, price: &str, fee: &str, hours: u32) -> Value {
    json!({
        "kind": "buy",
        "account_id": account,
        "instrument_id": instrument,
        "quantity": quantity,
        "price": price,
        "fee": fee,
        "occurred_at": hour(hours),
    })
}

fn sell(account: Uuid, instrument: Uuid, quantity: &str, price: &str, fee: &str, hours: u32) -> Value {
    json!({
        "kind": "sell",
        "account_id": account,
        "instrument_id": instrument,
        "quantity": quantity,
        "price": price,
        "fee": fee,
        "occurred_at": hour(hours),
    })
}

#[tokio::test]
async fn a_buy_books_what_it_cost_and_a_sale_what_it_brought_in() {
    let Some(world) = World::new("portfolio_money").await else { return };

    let bought = world.trade(buy(world.exchange, world.btc, "0.01", "60000", "1", 10)).await;
    assert_eq!(number(&bought["money"]), decimal("-601"), "a buy costs its price and its fee");
    assert_eq!(bought["currency"], "USDT", "a trade is in its account's currency");
    assert_eq!(bought["occurred_on"], "2026-10-20", "the day is the person's, not UTC's");
    assert_eq!(world.cash().await, decimal("399"));

    // The money is in the books as an entry the portfolio keeps: the account,
    // and the holding it became - not spending.
    let entry_id: Uuid = bought["entry_id"].as_str().expect("an entry").parse().expect("an id");
    let entry = austeris_ledger::repository::entry(&world.ledger_pool, world.owner, entry_id)
        .await
        .expect("reading")
        .expect("the entry");
    assert!(entry.kept);
    assert_eq!(entry.source.as_deref(), Some("portfolio"));
    assert_eq!(entry.description, "Buy 0.01 BTC");
    assert_eq!(entry.occurred_on, NaiveDate::from_ymd_opt(2026, 10, 20).expect("a day"));
    let sides: Vec<_> = entry.lines.iter().map(|line| (line.side, line.amount.normalize())).collect();
    assert_eq!(
        sides,
        [
            (austeris_ledger::model::LineSide::Account, decimal("-601")),
            (austeris_ledger::model::LineSide::Holding, decimal("601"))
        ]
    );

    let sold = world.trade(sell(world.exchange, world.btc, "0.004", "65000", "0.26", 36)).await;
    assert_eq!(number(&sold["money"]), decimal("259.74"), "a sale brings its price less its fee");
    assert_eq!(world.cash().await, decimal("658.74"));

    let (_, positions) = world.call("GET", "/portfolio/positions", None).await;
    assert_eq!(positions.as_array().expect("positions").len(), 1);
    assert_eq!(number(&positions[0]["quantity"]), decimal("0.006"));
    // As of the first day, only the buy had happened; before it, nothing.
    let (_, first_day) = world.call("GET", "/portfolio/positions?on=2026-10-20", None).await;
    assert_eq!(number(&first_day[0]["quantity"]), decimal("0.01"));
    let (_, before) = world.call("GET", "/portfolio/positions?on=2026-10-19", None).await;
    assert_eq!(before, json!([]));

    let (_, trades) = world.call("GET", "/portfolio/trades", None).await;
    let kinds: Vec<&str> = trades.as_array().expect("trades").iter().map(|trade| trade["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["sell", "buy"], "the latest first");
}

#[tokio::test]
async fn nobody_sells_what_they_did_not_hold_at_the_time() {
    let Some(world) = World::new("portfolio_oversold").await else { return };
    let bought = world.trade(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)).await;

    for (body, why) in [
        (sell(world.exchange, world.btc, "0.02", "61000", "0", 12), "more than was bought"),
        (sell(world.exchange, world.btc, "0.01", "61000", "0", 9), "before it was bought"),
    ] {
        let (status, refused) = world.call("POST", "/portfolio/trades", Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "a sale of {why} was recorded: {refused}");
        assert_eq!(refused["code"], "trade.oversold");
        assert_eq!(refused["params"]["symbol"], "BTC");
    }
    let (_, refused) = world
        .call("POST", "/portfolio/trades", Some(sell(world.exchange, world.btc, "0.02", "61000", "0", 12)))
        .await;
    assert_eq!(number(&refused["params"]["short"]), decimal("0.01"));
    // Nothing of a refused sale reached the books.
    assert_eq!(world.cash().await, decimal("400"));

    // Selling it all is fine, at the same instant even; and then the buy it
    // came from cannot go, nor become smaller.
    world.trade(sell(world.exchange, world.btc, "0.01", "61000", "0", 10)).await;
    let id = bought["id"].as_str().expect("an id");
    let (status, refused) = world.call("DELETE", &format!("/portfolio/trades/{id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "trade.oversold");
    let (status, _) = world
        .call(
            "PUT",
            &format!("/portfolio/trades/{id}"),
            Some(buy(world.exchange, world.btc, "0.005", "60000", "0", 10)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(world.cash().await, decimal("1010"), "a refused change moved money");
}

#[tokio::test]
async fn two_sales_of_the_same_coins_at_once_are_one_sale() {
    let Some(world) = World::new("portfolio_race").await else { return };
    world.trade(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)).await;

    // Each sells everything; whichever is second finds nothing left.
    let first = call(
        world.app(),
        world.owner,
        "POST",
        "/portfolio/trades",
        Some(sell(world.exchange, world.btc, "0.01", "61000", "0", 11)),
    );
    let second = call(
        world.app(),
        world.owner,
        "POST",
        "/portfolio/trades",
        Some(sell(world.exchange, world.btc, "0.01", "62000", "0", 12)),
    );
    let ((one, _), (two, _)) = tokio::join!(first, second);
    let mut statuses = [one, two];
    statuses.sort_unstable();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT], "the same coins were sold twice");
    let (_, positions) = world.call("GET", "/portfolio/positions", None).await;
    assert_eq!(positions, json!([]));
}

#[tokio::test]
async fn a_trade_changed_restates_its_money_and_one_deleted_takes_it_back() {
    let Some(world) = World::new("portfolio_change").await else { return };
    let bought = world.trade(buy(world.exchange, world.btc, "0.01", "60000", "1", 10)).await;
    let id = bought["id"].as_str().expect("an id").to_owned();

    let (status, changed) = world
        .call(
            "PUT",
            &format!("/portfolio/trades/{id}"),
            Some(buy(world.exchange, world.btc, "0.02", "59000", "2", 11)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["entry_id"], bought["entry_id"], "the same entry, restated");
    assert_eq!(number(&changed["money"]), decimal("-1182"));
    assert_eq!(world.cash().await, decimal("-182"));
    let entry_id: Uuid = changed["entry_id"].as_str().expect("an entry").parse().expect("an id");
    let revisions = austeris_ledger::repository::history(&world.ledger_pool, world.owner, entry_id)
        .await
        .expect("reading")
        .expect("a history");
    assert_eq!(revisions.last().and_then(|revision| revision.who.module.as_deref()), Some("portfolio"));

    // A buy said to be a transfer moves no money: its entry goes.
    let wallet = world.account(AccountKind::CryptoWallet, "Hardware wallet", "USD").await;
    world.trade(buy(world.exchange, world.btc, "0.05", "60000", "0", 9)).await;
    let (status, moved) = world
        .call(
            "PUT",
            &format!("/portfolio/trades/{id}"),
            Some(json!({
                "kind": "transfer", "account_id": world.exchange, "to_account_id": wallet, "instrument_id": world.btc,
                "quantity": "0.01", "fee": "0.0001", "occurred_at": "2026-10-20T11:00:00-03:00"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["entry_id"], Value::Null);
    assert_eq!(world.cash().await, decimal("-2000"), "only the other buy's money is left");
    // And back to a buy books it again.
    let (status, again) = world
        .call(
            "PUT",
            &format!("/portfolio/trades/{id}"),
            Some(buy(world.exchange, world.btc, "0.01", "60000", "1", 10)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(world.cash().await, decimal("-2601"));

    let (status, _) = world.call("DELETE", &format!("/portfolio/trades/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(world.cash().await, decimal("-2000"), "the deleted buy's money is still in the books");
    let (status, _) = world.call("GET", &format!("/portfolio/trades/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Its history is what is left of it: who made it, every change, and who
    // deleted it.
    let (status, history) = world.call("GET", &format!("/portfolio/trades/{id}/history"), None).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let actions: Vec<&str> = history
        .as_array()
        .expect("revisions")
        .iter()
        .map(|revision| revision["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["created", "changed", "changed", "changed", "deleted"]);
    assert_eq!(history[0]["who"], json!({ "kind": "person", "person_id": world.owner }));
    assert_eq!(history[1]["before"]["quantity"], "0.01");
    assert_eq!(history[1]["after"]["quantity"], "0.02");
    assert_eq!(history[4]["after"], Value::Null);
}

#[tokio::test]
async fn a_form_saved_untouched_is_no_change() {
    let Some(world) = World::new("portfolio_untouched").await else { return };
    let body = buy(world.exchange, world.btc, "0.01", "60000", "1", 10);
    let bought = world.trade(body.clone()).await;
    let id = bought["id"].as_str().expect("an id");
    let (status, _) = world.call("PUT", &format!("/portfolio/trades/{id}"), Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, history) = world.call("GET", &format!("/portfolio/trades/{id}/history"), None).await;
    assert_eq!(history.as_array().expect("revisions").len(), 1, "{history}");
}

#[tokio::test]
async fn a_transfer_moves_units_and_no_money() {
    let Some(world) = World::new("portfolio_transfer").await else { return };
    let wallet = world.account(AccountKind::CryptoWallet, "Hardware wallet", "USD").await;
    world.trade(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)).await;

    let sent = world
        .trade(json!({
            "kind": "transfer", "account_id": world.exchange, "to_account_id": wallet, "instrument_id": world.btc,
            "quantity": "0.01", "fee": "0.0001", "occurred_at": "2026-10-20T12:00:00-03:00", "note": "to cold storage"
        }))
        .await;
    assert_eq!(sent["money"], Value::Null);
    assert_eq!(sent["entry_id"], Value::Null);
    assert_eq!(world.cash().await, decimal("400"), "a transfer moved money");

    let (_, positions) = world.call("GET", "/portfolio/positions", None).await;
    let held: Vec<(String, Decimal)> = positions
        .as_array()
        .expect("positions")
        .iter()
        .map(|position| (position["account_id"].as_str().unwrap().to_owned(), number(&position["quantity"])))
        .collect();
    assert_eq!(held, [(wallet.to_string(), decimal("0.0099"))], "the network's fee did not arrive");

    // What arrived can be sent on; what did not, cannot.
    let (status, refused) = world
        .call(
            "POST",
            "/portfolio/trades",
            Some(json!({
                "kind": "transfer", "account_id": wallet, "to_account_id": world.exchange, "instrument_id": world.btc,
                "quantity": "0.01", "occurred_at": "2026-10-20T13:00:00-03:00"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
}

/// A ledger nobody can reach.
#[derive(Clone)]
struct Down;

impl Books for Down {
    fn accounts(&self, _owner: Uuid) -> impl Future<Output = Result<Vec<Account>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn cash(&self, _owner: Uuid, _as_of: NaiveDate, _currency: &str) -> impl Future<Output = Result<Vec<Cash>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn post(&self, _request: PostEntryRequest) -> impl Future<Output = Result<Uuid, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn restate(&self, _request: RestateEntryRequest) -> impl Future<Output = Result<Uuid, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn withdraw(&self, _owner: Uuid, _key: &str) -> impl Future<Output = Result<bool, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
    fn rate(&self, _base: &str, _quote: &str, _on: NaiveDate) -> impl Future<Output = Result<Option<Rate>, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger is down")))
    }
}

/// The ledger answering what accounts there are, and down for posting.
#[derive(Clone)]
struct DownForPosting(Ledger);

impl Books for DownForPosting {
    fn accounts(&self, owner: Uuid) -> impl Future<Output = Result<Vec<Account>, Status>> {
        self.0.accounts(owner)
    }
    fn cash(&self, owner: Uuid, as_of: NaiveDate, currency: &str) -> impl Future<Output = Result<Vec<Cash>, Status>> {
        self.0.cash(owner, as_of, currency)
    }
    fn post(&self, _request: PostEntryRequest) -> impl Future<Output = Result<Uuid, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger went down")))
    }
    fn restate(&self, _request: RestateEntryRequest) -> impl Future<Output = Result<Uuid, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger went down")))
    }
    fn withdraw(&self, _owner: Uuid, _key: &str) -> impl Future<Output = Result<bool, Status>> {
        std::future::ready(Err(Status::unavailable("the ledger went down")))
    }
    fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> impl Future<Output = Result<Option<Rate>, Status>> {
        self.0.rate(base, quote, on)
    }
}

#[tokio::test]
async fn a_trade_is_refused_where_it_cannot_happen() {
    let Some(world) = World::new("portfolio_refused").await else { return };
    let bank = world.account(AccountKind::Bank, "Bank", "USD").await;
    let closed = world.account(AccountKind::Brokerage, "Old broker", "USD").await;
    austeris_ledger::repository::set_account_closed(&world.ledger_pool, world.owner, closed, true)
        .await
        .expect("closing");
    let usdt = world.instrument(Kind::Crypto, "USDT").await;
    let dollar = world.instrument(Kind::Fx, "EUR").await;

    let cases = [
        (buy(bank, world.btc, "1", "1", "0", 10), "trade.not_a_holding_account"),
        (buy(closed, world.btc, "1", "1", "0", 10), "trade.closed_account"),
        (buy(Uuid::new_v4(), world.btc, "1", "1", "0", 10), "trade.unknown_account"),
        (buy(world.exchange, Uuid::new_v4(), "1", "1", "0", 10), "trade.unknown_instrument"),
        // A stablecoin the person keeps an account in is money, not a coin.
        (buy(world.exchange, usdt, "100", "1", "0", 10), "trade.money_not_holding"),
        (buy(world.exchange, dollar, "100", "1.1", "0", 10), "trade.money_not_holding"),
        (sell(world.exchange, world.btc, "0.0001", "100", "0.01", 10), "trade.fee_too_large"),
        (
            json!({
                "kind": "transfer", "account_id": world.exchange, "to_account_id": closed, "instrument_id": world.btc,
                "quantity": "1", "occurred_at": "2026-10-20T10:00:00-03:00"
            }),
            "trade.closed_account",
        ),
    ];
    for (body, code) in cases {
        let (status, refused) = world.call("POST", "/portfolio/trades", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} was recorded: {refused}");
        assert_eq!(refused["code"], code, "{body}: {refused}");
    }

    let wallet = world.account(AccountKind::CryptoWallet, "Wallet", "USD").await;
    let (status, refused) = world
        .call(
            "POST",
            "/portfolio/trades",
            Some(json!({
                "kind": "transfer", "account_id": world.exchange, "to_account_id": wallet, "instrument_id": world.btc,
                "quantity": "0.01", "fee": "0.01", "occurred_at": "2026-10-20T10:00:00-03:00"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["code"], "trade.fee_too_large");

    // A ledger that does not answer records nothing, here or there.
    let (status, refused) = call(
        routes::router(world.pool.clone(), Down, world.market()),
        world.owner,
        "POST",
        "/portfolio/trades",
        Some(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
    assert_eq!(refused["code"], "portfolio.ledger_unavailable");
    let (status, refused) = call(
        routes::router(world.pool.clone(), DownForPosting(world.books()), world.market()),
        world.owner,
        "POST",
        "/portfolio/trades",
        Some(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
    let (_, trades) = world.call("GET", "/portfolio/trades", None).await;
    assert_eq!(trades, json!([]), "a trade was kept without its money");
}

#[tokio::test]
async fn the_value_is_the_money_at_the_ledgers_rate_and_the_coins_at_the_markets_price() {
    let Some(world) = World::new("portfolio_value").await else { return };
    let eth = world.instrument(Kind::Crypto, "ETH").await;
    world.trade(buy(world.exchange, world.btc, "0.01", "60000", "1", 10)).await;
    world.trade(buy(world.exchange, eth, "0.1", "2500", "0", 11)).await;
    world.price(world.btc, "USD", Utc::now() - chrono::Duration::minutes(5), "62000").await;
    // USDT is money on the exchange, worth what the ledger's rate says.
    austeris_ledger::repository::record_rate(&world.ledger_pool, "USDT", "USD", Utc::now().date_naive(), decimal("0.999"), "manual")
        .await
        .expect("a rate");

    let (status, value) = world.call("GET", "/portfolio/value?currency=usd", None).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["currency"], "USD");
    let exchange = &value["accounts"][0];
    assert_eq!(exchange["name"], "Exchange");
    let holdings = exchange["holdings"].as_array().expect("holdings");
    let symbols: Vec<&str> = holdings.iter().map(|holding| holding["symbol"].as_str().unwrap()).collect();
    assert_eq!(symbols, ["USDT", "BTC", "ETH"], "the money first, then the coins");
    assert_eq!(holdings[0]["kind"], "cash");
    assert_eq!(number(&holdings[0]["quantity"]), decimal("149"));
    assert_eq!(number(&holdings[0]["value"]), decimal("148.851"));
    assert_eq!(number(&holdings[1]["value"]), decimal("620"));
    assert_eq!(holdings[2]["value"], Value::Null, "a coin with no price was given one");
    assert_eq!(number(&exchange["total"]), decimal("768.851"));
    assert_eq!(number(&value["total"]), decimal("768.851"));
    assert_eq!(value["unvalued"], json!(["ETH"]), "what the total leaves out is named");

    // In a currency nothing is priced in, and no rate leads to, the coins have
    // no value and say so.
    let (_, in_euros) = world.call("GET", "/portfolio/value?currency=EUR", None).await;
    assert_eq!(in_euros["unvalued"], json!(["BTC", "ETH", "USDT"]));
    assert_eq!(number(&in_euros["total"]), Decimal::ZERO);

    // In guaranies, which no source quotes a coin in: the dollar price at the
    // ledger's rate for the day - and the stablecoin through the dollar.
    austeris_ledger::repository::record_rate(&world.ledger_pool, "USD", "PYG", Utc::now().date_naive(), decimal("7000"), "bcp")
        .await
        .expect("a rate");
    let (_, in_guaranies) = world.call("GET", "/portfolio/value?currency=PYG", None).await;
    let holdings = in_guaranies["accounts"][0]["holdings"].as_array().expect("holdings");
    assert_eq!(number(&holdings[1]["price"]), decimal("434000000"), "{in_guaranies}");
    assert_eq!(number(&holdings[1]["value"]), decimal("4340000"));
    assert_eq!(number(&holdings[0]["value"]), decimal("1041957"), "149 USDT at 0.999 dollars at 7000 guaranies");
    assert_eq!(in_guaranies["unvalued"], json!(["ETH"]));

    let (status, _) = world.call("GET", "/portfolio/value?currency=dollars", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_snapshot_keeps_what_was_and_is_set_beside_what_is() {
    let Some(world) = World::new("portfolio_snapshots").await else { return };
    let morning = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).single().expect("an instant");
    world.price(world.btc, "USDT", morning, "60000").await;
    world.trade(buy(world.exchange, world.btc, "0.01", "60000", "0", 8)).await;

    let (status, taken) = world
        .call(
            "POST",
            "/portfolio/snapshots",
            Some(json!({ "currency": "USDT", "note": "before the second buy" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    let id = taken["id"].as_str().expect("an id").to_owned();
    assert_eq!(number(&taken["total"]), decimal("1000"), "400 USDT and 0.01 BTC at 60000");

    // Later: more bought, the price up.
    world.trade(buy(world.exchange, world.btc, "0.005", "62000", "0", 13)).await;
    world.price(world.btc, "USDT", Utc::now() - chrono::Duration::minutes(1), "64000").await;

    let (status, compared) = world.call("GET", &format!("/portfolio/snapshots/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{compared}");
    assert_eq!(compared["snapshot"]["note"], "before the second buy");
    let btc = compared["differences"]
        .as_array()
        .expect("differences")
        .iter()
        .find(|line| line["symbol"] == "BTC")
        .expect("BTC");
    assert_eq!(number(&btc["quantity_then"]), decimal("0.01"));
    assert_eq!(number(&btc["quantity_now"]), decimal("0.015"));
    assert_eq!(number(&btc["value_then"]), decimal("600"));
    assert_eq!(number(&btc["value_now"]), decimal("960"));
    let money = compared["differences"]
        .as_array()
        .expect("differences")
        .iter()
        .find(|line| line["kind"] == "cash")
        .expect("the money");
    assert_eq!(number(&money["quantity_then"]), decimal("400"));
    assert_eq!(number(&money["quantity_now"]), decimal("90"));

    // A snapshot of a day past: before anything was bought.
    let (status, earlier) = world
        .call("POST", "/portfolio/snapshots", Some(json!({ "currency": "USDT", "on": "2026-10-19" })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{earlier}");
    assert_eq!(earlier["on"], "2026-10-19");
    assert_eq!(number(&earlier["total"]), decimal("1000"), "only the money was there");
    let earlier_id = earlier["id"].as_str().expect("an id");
    let (_, between) = world.call("GET", &format!("/portfolio/snapshots/{earlier_id}?against={id}"), None).await;
    assert_eq!(between["against_id"], json!(id));
    let btc = between["differences"]
        .as_array()
        .expect("lines")
        .iter()
        .find(|line| line["symbol"] == "BTC")
        .expect("BTC");
    assert_eq!(number(&btc["quantity_then"]), Decimal::ZERO);
    assert_eq!(number(&btc["quantity_now"]), decimal("0.01"));

    let (_, listed) = world.call("GET", "/portfolio/snapshots", None).await;
    let moments: Vec<&str> = listed
        .as_array()
        .expect("snapshots")
        .iter()
        .map(|snapshot| snapshot["id"].as_str().unwrap())
        .collect();
    assert_eq!(moments, [id.as_str(), earlier_id], "the latest moment first");

    let (status, _) = world.call("DELETE", &format!("/portfolio/snapshots/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = world.call("GET", &format!("/portfolio/snapshots/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = world.call("POST", "/portfolio/snapshots", Some(json!({ "on": "2999-01-01" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_trade_written_without_saying_whose_is_refused() {
    let Some(world) = World::new("portfolio_actor").await else { return };
    // The service's own connection, forgetting who it writes for.
    let error = sqlx::query(
        "INSERT INTO trades (owner_id, kind, account_id, to_account_id, instrument_id, quantity, occurred_at, occurred_on)
         VALUES ($1, 'transfer', $2, $3, $4, 1, now(), current_date)",
    )
    .bind(world.owner)
    .bind(world.exchange)
    .bind(Uuid::new_v4())
    .bind(world.btc)
    .execute(&world.pool)
    .await
    .expect_err("a trade with no one behind it");
    assert!(error.to_string().contains("without saying on whose behalf"), "{error}");

    // Said, it is that person's - and is still held to the positions' rule.
    let mut transaction = Actor::Person(world.owner).begin(&world.pool).await.expect("a transaction");
    let refused = sqlx::query(
        "INSERT INTO trades (owner_id, kind, account_id, to_account_id, instrument_id, quantity, occurred_at, occurred_on)
         VALUES ($1, 'transfer', $2, $3, $4, 1, now(), current_date)",
    )
    .bind(world.owner)
    .bind(world.exchange)
    .bind(Uuid::new_v4())
    .bind(world.btc)
    .execute(&mut *transaction)
    .await
    .expect_err("sent coins never held");
    assert_eq!(refused.as_database_error().and_then(sqlx::error::DatabaseError::code).as_deref(), Some("AUP01"));
}

#[tokio::test]
async fn the_schema_rolls_back_only_when_nothing_would_be_lost() {
    let Some(world) = World::new("portfolio_migrate").await else { return };
    austeris_common::migrate::undo(&world.pool, &MIGRATOR, 0)
        .await
        .expect("rolling back an empty portfolio");
    austeris_common::migrate::run(&world.pool, &MIGRATOR).await.expect("migrating again");

    world.trade(buy(world.exchange, world.btc, "0.01", "60000", "0", 10)).await;
    let error = austeris_common::migrate::undo(&world.pool, &MIGRATOR, 0)
        .await
        .expect_err("rolled back over a trade");
    assert!(format!("{error:#}").contains("has trades"), "{error:#}");
}

#[tokio::test]
async fn a_buy_is_not_kept_without_its_money_in_the_books() {
    let Some(world) = World::new("portfolio_unbooked").await else { return };
    // A path that forgot to ask the ledger: the schema refuses it at commit.
    let mut transaction = Actor::Person(world.owner).begin(&world.pool).await.expect("a transaction");
    sqlx::query(
        "INSERT INTO trades (owner_id, kind, account_id, instrument_id, quantity, price, currency, occurred_at, occurred_on)
         VALUES ($1, 'buy', $2, $3, 1, 100, 'USDT', now(), current_date)",
    )
    .bind(world.owner)
    .bind(world.exchange)
    .bind(world.btc)
    .execute(&mut *transaction)
    .await
    .expect("writing it is not yet the problem");
    let error = transaction.commit().await.expect_err("a buy was kept with no money in the books");
    assert!(error.to_string().contains("the books have no entry"), "{error}");
}
