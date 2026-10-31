//! `austeris import legacy` from end to end: the binary serving a database of
//! its own, the binary signing in and importing a real `pg_dump` of the 2025
//! tracker's schema, and the books it leaves, read back through the API -
//! twice, because the second run must change nothing.
//!
//! Without `AUSTERIS_DATABASE_URL` this skips itself, like every database
//! test, and the CI job that owns them fails when it does.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use serde_json::Value;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// The database this test owns, dropped and made again on every run.
const DATABASE: &str = "austeris_test_legacy_import";

/// Who the installation's first account is.
const OWNER: &str = "owner@legacy.test";

fn decimal(text: &str) -> Decimal {
    Decimal::from_str(text).unwrap_or_else(|_| panic!("`{text}` is not a decimal"))
}

/// The decimal a JSON field carries as a string.
fn number(value: &Value) -> Decimal {
    decimal(value.as_str().unwrap_or_else(|| panic!("{value} is not a decimal string")))
}

/// The dump, as `tools/legacy-fixture.sh` made it.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-2025.sql")
}

/// A running installation, stopped when this goes.
struct Serving {
    child: Child,
    url: String,
    password: String,
    home: PathBuf,
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Serving {
    /// `austeris serve` on a database of its own, once it answers.
    async fn start(database_url: &str) -> Self {
        let admin = sqlx::PgPool::connect(database_url).await.expect("connecting to the test server");
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS {DATABASE} WITH (FORCE)")))
            .execute(&admin)
            .await
            .expect("dropping the last run's database");
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {DATABASE}")))
            .execute(&admin)
            .await
            .expect("making this run's database");
        admin.close().await;
        let (server, _) = database_url.rsplit_once('/').expect("a database URL names its database");

        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port")
            .port();
        let home = std::env::temp_dir().join(format!("austeris-legacy-import-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("a home for the session");
        let mut child = Command::new(env!("CARGO_BIN_EXE_austeris"))
            .arg("serve")
            .env("AUSTERIS_DATABASE_URL", format!("{server}/{DATABASE}"))
            .env("AUSTERIS_BIND", format!("127.0.0.1:{port}"))
            .env("AUSTERIS_FIRST_USER", OWNER)
            .env("RUST_LOG", "warn")
            .env_remove("AUSTERIS_DEMO")
            .env_remove("AUSTERIS_INBOX")
            .env_remove("AUSTERIS_CMC_API_KEY")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("starting austeris serve");

        // The first account's password is said once, on stdout; what follows
        // is drained so the server never blocks on a full pipe.
        let stdout = child.stdout.take().expect("its stdout");
        let (said, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(password) = line.trim().strip_prefix("password:") {
                    let _ = said.send(password.trim().to_owned());
                }
            }
        });
        let password = heard.recv_timeout(Duration::from_secs(180)).expect("the first account's password");
        let serving = Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            password,
            home,
        };

        let client = austeris_common::http::client();
        let deadline = Instant::now() + Duration::from_secs(120);
        while client.get(format!("{}/healthz", serving.url)).send().await.is_err() {
            assert!(Instant::now() < deadline, "austeris serve never answered");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        serving
    }

    /// Runs the binary as a client of this installation, with a session of
    /// this test's own.
    fn run(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_austeris"))
            .args(args)
            .env("AUSTERIS_URL", &self.url)
            .env("APPDATA", &self.home)
            .env("XDG_STATE_HOME", &self.home)
            .output()
            .expect("running austeris");
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        (output.status.success(), text)
    }

    /// Reads the API as the signed-in owner.
    async fn get(&self, path: &str) -> Value {
        let session = std::fs::read_to_string(self.home.join("austeris").join("session")).expect("a session");
        let response = austeris_common::http::client()
            .get(format!("{}{path}", self.url))
            .header(reqwest::header::COOKIE, session.trim())
            .send()
            .await
            .expect("an answer");
        assert!(response.status().is_success(), "{path}: {}", response.status());
        serde_json::from_str(&response.text().await.expect("a body")).expect("JSON")
    }
}

/// The account of a name.
fn account<'a>(accounts: &'a Value, name: &str) -> &'a Value {
    accounts
        .as_array()
        .expect("accounts")
        .iter()
        .find(|account| account["name"] == name)
        .unwrap_or_else(|| panic!("no account {name} in {accounts}"))
}

#[tokio::test]
async fn the_trackers_records_are_brought_once_and_where_they_part_is_said() {
    let Ok(database_url) = std::env::var("AUSTERIS_DATABASE_URL") else {
        eprintln!("{SKIP}");
        return;
    };
    let serving = Serving::start(&database_url).await;
    let (signed_in, said) = serving.run(&["login", "--email", OWNER, "--password", &serving.password]);
    assert!(signed_in, "{said}");

    let dump = fixture();
    let dump = dump.to_str().expect("a path");
    let (imported, report) = serving.run(&["import", "legacy", dump, "--offset", "-03:00"]);
    assert!(imported, "{report}");
    for expected in [
        "Binance: opened with 2752.75 USDT",
        "Ledger Nano: opened with 1460 USDT",
        "Trades: 6 brought (6 new, 0 already there), 2 left out",
        "#7 2025-04-02 Binance: buy 10 SOL - it was bought for nothing",
        "#8 2025-04-03 Binance: sell 4 SOL - it sells 4 SOL more",
        "#3 sell 1701.7 USDT - the money of #2 (buy 0.02 BTC)",
        "Snapshots: 3 taken, one a day",
        "#2 is of the same day as #3",
        "Not brought, nothing happened on them: Bybit",
    ] {
        assert!(report.contains(expected), "the report does not say `{expected}`:\n{report}");
    }

    // The wallets are accounts in USDT, opening with the money no trade
    // explains and what the trades needed besides.
    let accounts = serving.get("/api/v1/ledger/accounts").await;
    let binance = account(&accounts, "Binance");
    assert_eq!(binance["kind"], "crypto_wallet");
    assert_eq!(binance["currency"], "USDT");
    assert_eq!(number(&binance["opening_balance"]), decimal("2752.75"));
    assert_eq!(number(&account(&accounts, "Ledger Nano")["opening_balance"]), decimal("1460"));
    assert_eq!(
        accounts.as_array().expect("accounts").len(),
        2,
        "an unused wallet became an account: {accounts}"
    );

    the_trades_are_the_transactions(&serving).await;
    the_books_are_worth_what_the_trades_and_prices_say(&serving).await;

    // Again: nothing new.
    let (again, report) = serving.run(&["import", "legacy", dump, "--offset", "-03:00"]);
    assert!(again, "{report}");
    assert!(report.contains("Trades: 6 brought (0 new, 6 already there)"), "{report}");
    assert!(report.contains("Snapshots: 3 taken, one a day (3 already there)"), "{report}");
    assert!(report.contains("Binance: already there, opening with 2752.75 USDT"), "{report}");
    assert_eq!(serving.get("/api/v1/portfolio/trades").await.as_array().expect("trades").len(), 6);
    assert_eq!(serving.get("/api/v1/portfolio/snapshots").await.as_array().expect("snapshots").len(), 3);
    assert_eq!(serving.get("/api/v1/ledger/accounts").await.as_array().expect("accounts").len(), 2);
}

/// The trades, each named by its transaction, their notes and amounts whole,
/// and their money in the books.
async fn the_trades_are_the_transactions(serving: &Serving) {
    let trades = serving.get("/api/v1/portfolio/trades").await;
    let trades = trades.as_array().expect("trades");
    let keys: std::collections::BTreeSet<String> = trades
        .iter()
        .map(|trade| trade["idempotency_key"].as_str().expect("a key").to_owned())
        .collect();
    let expected: std::collections::BTreeSet<String> = [2, 4, 5, 9, 10, 11].iter().map(|id| format!("austeris-2025:transaction:{id}")).collect();
    assert_eq!(keys, expected);
    let of = |id: u32| {
        trades
            .iter()
            .find(|trade| trade["idempotency_key"] == format!("austeris-2025:transaction:{id}").as_str())
            .unwrap_or_else(|| panic!("no trade of #{id}"))
    };
    let ether = of(4);
    assert_eq!(ether["note"], "dip buy \u{2615} caf\u{e9}\twith a tab", "the note crossed every escape whole");
    assert_eq!(ether["occurred_on"], "2025-03-10");
    assert_eq!(number(&ether["money"]), decimal("-1051.05"));
    assert_eq!(of(10)["note"], "cold storage\\vault");
    // A float's sum is the decimal it was.
    assert_eq!(number(&of(11)["quantity"]), decimal("0.3"));

    let balances = serving.get("/api/v1/ledger/balances?as_of=2025-04-15").await;
    let cash: Vec<(String, Decimal)> = balances["accounts"]
        .as_array()
        .expect("balances")
        .iter()
        .map(|balance| (balance["name"].as_str().unwrap_or_default().to_owned(), number(&balance["amount"])))
        .collect();
    assert!(cash.contains(&("Binance".to_owned(), decimal("739.26"))), "{balances}");
    assert!(cash.contains(&("Ledger Nano".to_owned(), Decimal::ZERO)), "{balances}");
}

/// One snapshot a day, and the books worth what the trades hold at the
/// tracker's prices.
async fn the_books_are_worth_what_the_trades_and_prices_say(serving: &Serving) {
    let snapshots = serving.get("/api/v1/portfolio/snapshots").await;
    let days: Vec<&str> = snapshots
        .as_array()
        .expect("snapshots")
        .iter()
        .map(|snapshot| snapshot["on"].as_str().expect("a day"))
        .collect();
    assert_eq!(days, ["2025-04-15", "2025-03-20", "2025-03-06"]);

    let value = serving.get("/api/v1/portfolio/value?on=2025-04-15&currency=USD").await;
    // 0.025 BTC at 83000.75, 0.6 ETH at 1590.5, and 739.26 USDT at 0.9999.
    let worth = decimal("2075.01875") + decimal("954.3") + decimal("739.186074");
    let total = number(&value["total"]);
    assert!((total - worth).abs() < decimal("0.01"), "the books are worth {total}: {value}");
    assert_eq!(value["unvalued"], serde_json::json!([]));

    let instruments = serving.get("/api/v1/market/instruments").await;
    let bitcoin = instruments
        .as_array()
        .expect("instruments")
        .iter()
        .find(|instrument| instrument["symbol"] == "BTC")
        .expect("BTC")["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let prices = serving
        .get(&format!(
            "/api/v1/market/prices/{bitcoin}/history?from=2025-01-01T00:00:00Z&to=2025-12-31T00:00:00Z"
        ))
        .await;
    assert_eq!(prices.as_array().expect("prices").len(), 4);
    assert_eq!(prices[0]["source"], "austeris-2025");
}

#[test]
fn a_dry_run_needs_no_installation_and_says_what_would_be() {
    let output = Command::new(env!("CARGO_BIN_EXE_austeris"))
        .args(["import", "legacy"])
        .arg(fixture())
        .args(["--offset", "-03:00", "--dry-run"])
        .env("AUSTERIS_URL", "http://127.0.0.1:9")
        .env("APPDATA", Path::new("nowhere-at-all"))
        .env("XDG_STATE_HOME", Path::new("nowhere-at-all"))
        .output()
        .expect("running austeris");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{report}{}", String::from_utf8_lossy(&output.stderr));
    assert!(report.contains("Binance: opens with 2752.75 USDT"), "{report}");
    assert!(report.contains("Nothing was written"), "{report}");
}
