//! Writing the plan into the books, through the installation's API.
//!
//! Like `add` and `import`, this is a client of the gateway and nothing else
//! (ADR 0001): accounts through the ledger's endpoints, instruments and prices
//! through the market's, trades and snapshots through the portfolio's - with
//! every rule those apply. Each trade and snapshot is named by its key, so a
//! run that stops half way is finished by running it again.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};

use super::plan::{AccountPlan, MONEY, Plan, SOURCE};
use super::tracker::Holdings;
use crate::add::{base_url, read_session};

/// The installation, as the signed-in person.
pub struct Api {
    client: reqwest::Client,
    base: String,
    session: String,
}

/// How many times a refusal to go faster is waited out before giving up.
const PATIENCE: u32 = 5;

/// The most prices one call records - the market's own ceiling.
const PRICES_PER_CALL: usize = 5_000;

impl Api {
    /// The installation `AUSTERIS_URL` names, with the session `austeris login`
    /// kept.
    ///
    /// # Errors
    ///
    /// Returns an error when nobody is signed in.
    pub fn signed_in() -> Result<Self> {
        Ok(Self {
            client: austeris_common::http::client(),
            base: base_url(),
            session: read_session()?,
        })
    }

    /// Asks, and reads the answer. Asked to slow down, it waits out the
    /// gateway's minute and asks again.
    async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<(StatusCode, Value)> {
        let mut waited = 0;
        loop {
            let mut request = self
                .client
                .request(method.clone(), format!("{}{path}", self.base))
                .header(reqwest::header::COOKIE, &self.session);
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await.with_context(|| format!("reaching austeris at {}", self.base))?;
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS && waited < PATIENCE {
                waited += 1;
                eprintln!("  austeris asks to slow down; waiting a minute");
                tokio::time::sleep(Duration::from_secs(61)).await;
                continue;
            }
            if status == StatusCode::UNAUTHORIZED {
                bail!("that session is no longer good; run `austeris login` again");
            }
            let text = response.text().await.context("reading what austeris answered")?;
            let json = serde_json::from_str(&text).unwrap_or(Value::String(text));
            return Ok((status, json));
        }
    }

    /// Asks, and expects a yes.
    async fn ok(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let (status, json) = self.call(method, path, body).await?;
        if !status.is_success() {
            bail!("{path}: {}", said(&json));
        }
        Ok(json)
    }
}

/// The sentence an error answer carries.
fn said(json: &Value) -> String {
    json["message"].as_str().map_or_else(|| json.to_string(), ToOwned::to_owned)
}

/// An account in the books, as much as the import needs of it.
#[derive(Debug, Clone, Deserialize)]
struct Account {
    id: String,
    name: String,
    kind: String,
    currency: String,
    #[serde(with = "rust_decimal::serde::str")]
    opening_balance: Decimal,
    closed_at: Option<String>,
}

impl Account {
    /// Whether the import can bring a wallet into it: a broker's or a
    /// wallet's account, kept in USDT, and open.
    fn fits(&self) -> bool {
        austeris_portfolio::HOLDING_KINDS.contains(&self.kind.as_str()) && self.currency.eq_ignore_ascii_case(MONEY) && self.closed_at.is_none()
    }
}

/// The account a wallet went into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The wallet.
    pub wallet_id: i64,
    /// The account.
    pub id: String,
    /// Its name in the books.
    pub name: String,
    /// Made by this run, rather than found.
    pub created: bool,
    /// What it opens with in the books.
    pub opening: Decimal,
}

/// What the books hold on the import's accounts, and what it is worth.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Books {
    /// By account and ticker.
    pub lines: BTreeMap<(String, String), Decimal>,
    /// By ticker.
    pub holdings: Holdings,
    /// What every line with a value adds up to, in dollars.
    pub total: Decimal,
    /// Tickers with no value.
    pub unvalued: Vec<String>,
}

/// A valuation, as much as the import reads of one.
#[derive(Debug, Deserialize)]
struct Valuation {
    accounts: Vec<AccountValue>,
}

#[derive(Debug, Deserialize)]
struct AccountValue {
    account_id: String,
    holdings: Vec<Holding>,
}

#[derive(Debug, Deserialize)]
struct Holding {
    symbol: String,
    #[serde(with = "rust_decimal::serde::str")]
    quantity: Decimal,
    #[serde(default, with = "rust_decimal::serde::str_option")]
    value: Option<Decimal>,
}

impl Books {
    /// What a valuation says of the import's accounts.
    fn of(json: Value, accounts: &[Placed]) -> Result<Self> {
        let valuation: Valuation = serde_json::from_value(json).context("reading a valuation")?;
        let mut books = Self::default();
        for account in valuation
            .accounts
            .iter()
            .filter(|account| accounts.iter().any(|placed| placed.id == account.account_id))
        {
            for holding in &account.holdings {
                let symbol = holding.symbol.to_uppercase();
                *books.lines.entry((account.account_id.clone(), symbol.clone())).or_default() += holding.quantity;
                *books.holdings.entry(symbol.clone()).or_default() += holding.quantity;
                match holding.value {
                    Some(value) => books.total += value,
                    None if !books.unvalued.contains(&symbol) => books.unvalued.push(symbol),
                    None => {}
                }
            }
        }
        books.holdings.retain(|_, amount| !amount.is_zero());
        books.holdings.values_mut().for_each(|amount| *amount = amount.normalize());
        books.total = books.total.normalize();
        books.unvalued.sort();
        Ok(books)
    }
}

/// A trade the installation would not record, and what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The transaction.
    pub legacy_id: i64,
    /// What the installation said.
    pub why: String,
}

/// What writing the plan came to.
#[derive(Debug, Clone, Default)]
pub struct Written {
    /// Where each wallet went.
    pub accounts: Vec<Placed>,
    /// How many instruments there are for it.
    pub instruments: usize,
    /// How many prices were recorded.
    pub prices: usize,
    /// How many days of USDT's rate were recorded.
    pub rates: usize,
    /// Trades recorded now.
    pub new: usize,
    /// Trades recorded by an earlier run.
    pub already: usize,
    /// Trades the installation refused.
    pub refused: Vec<Refused>,
    /// What the books say on each snapshot's day, in the plan's order.
    pub snapshots: Vec<Books>,
    /// Snapshots taken by an earlier run.
    pub snapshots_already: usize,
    /// What the books say at the end of the records.
    pub end: Option<Books>,
}

/// Writes the plan.
///
/// # Errors
///
/// Returns an error when the installation cannot be reached, refuses
/// something the import cannot go on without, or answers what it cannot read.
/// Running again picks up where it stopped.
pub async fn write(plan: &Plan, api: &Api) -> Result<Written> {
    let mut written = Written::default();

    eprintln!("Opening {} accounts", plan.accounts.len());
    let existing: Vec<Account> = serde_json::from_value(api.ok(Method::GET, "/api/v1/ledger/accounts", None).await?).context("reading the accounts")?;
    for account in &plan.accounts {
        written.accounts.push(place(api, &existing, account).await?);
    }
    let instruments = instruments(api, plan, &mut written).await?;
    prices(api, plan, &instruments, &mut written).await?;
    trades(api, plan, &instruments, &mut written).await?;
    snapshots(api, plan, &mut written).await?;

    if let Some(day) = plan.end.day {
        let valuation = api.ok(Method::GET, &format!("/api/v1/portfolio/value?on={day}&currency=USD"), None).await?;
        written.end = Some(Books::of(valuation, &written.accounts)?);
    }
    Ok(written)
}

/// Names each instrument in the market, and binds it to `CoinMarketCap` by
/// the id the tracker knew it by. Answers each ticker's instrument.
async fn instruments(api: &Api, plan: &Plan, written: &mut Written) -> Result<BTreeMap<String, String>> {
    eprintln!("Naming {} instruments", plan.instruments.len());
    let mut instruments: BTreeMap<String, String> = BTreeMap::new();
    for instrument in &plan.instruments {
        let made = api
            .ok(
                Method::POST,
                "/api/v1/market/instruments",
                Some(&json!({ "kind": "crypto", "symbol": instrument.symbol, "name": instrument.name, "decimals": instrument.decimals })),
            )
            .await?;
        let id = made["id"].as_str().context("an instrument without an id")?.to_owned();
        if let Some(cmc_id) = instrument.cmc_id {
            // So the coin is priced again the day a CoinMarketCap key is set.
            api.ok(
                Method::POST,
                &format!("/api/v1/market/instruments/{id}/sources"),
                Some(&json!({ "source": austeris_market::source::COINMARKETCAP, "external_id": cmc_id.to_string() })),
            )
            .await?;
        }
        instruments.insert(instrument.symbol.clone(), id);
        written.instruments += 1;
    }
    Ok(instruments)
}

/// Records the tracker's prices in the market, and what USDT was worth in
/// dollars in the ledger.
async fn prices(api: &Api, plan: &Plan, instruments: &BTreeMap<String, String>, written: &mut Written) -> Result<()> {
    eprintln!("Recording {} prices and {} days of USDT in dollars", plan.prices.len(), plan.rates.len());
    for chunk in plan.prices.chunks(PRICES_PER_CALL) {
        let prices: Vec<Value> = chunk
            .iter()
            .filter_map(|price| {
                let id = instruments.get(&price.symbol)?;
                Some(json!({ "instrument_id": id, "price": price.price.to_string(), "currency": "USD", "observed_at": price.at }))
            })
            .collect();
        if prices.is_empty() {
            continue;
        }
        let recorded = api
            .ok(Method::POST, "/api/v1/market/prices", Some(&json!({ "source": SOURCE, "prices": prices })))
            .await?;
        written.prices += recorded["recorded"].as_u64().and_then(|count| usize::try_from(count).ok()).unwrap_or_default();
    }
    for rate in &plan.rates {
        api.ok(
            Method::POST,
            "/api/v1/ledger/rates",
            Some(&json!({ "base": MONEY, "quote": "USD", "on_date": rate.day, "rate": rate.rate.to_string() })),
        )
        .await?;
        written.rates += 1;
    }
    Ok(())
}

/// Records the trades, each under its key.
async fn trades(api: &Api, plan: &Plan, instruments: &BTreeMap<String, String>, written: &mut Written) -> Result<()> {
    eprintln!("Recording {} trades", plan.trades.len());
    for trade in &plan.trades {
        let (Some(account), Some(instrument)) = (
            written.accounts.iter().find(|placed| placed.wallet_id == trade.wallet_id),
            instruments.get(&trade.symbol),
        ) else {
            bail!("the trade #{} has no account or instrument to be recorded on", trade.legacy_id);
        };
        let body = json!({
            "kind": if trade.kind == austeris_portfolio::model::TradeKind::Buy { "buy" } else { "sell" },
            "account_id": account.id,
            "instrument_id": instrument,
            "quantity": trade.quantity.to_string(),
            "price": trade.price.to_string(),
            "fee": trade.fee.to_string(),
            "occurred_at": trade.at.with_timezone(&trade.offset).to_rfc3339(),
            "note": trade.note,
            "idempotency_key": trade.key,
        });
        let (status, answer) = api.call(Method::POST, "/api/v1/portfolio/trades", Some(&body)).await?;
        match status {
            StatusCode::CREATED => written.new += 1,
            StatusCode::OK => written.already += 1,
            // The installation's own rules, said in its own words: a sale
            // of coins an account of the person's already sold, say.
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT => written.refused.push(Refused {
                legacy_id: trade.legacy_id,
                why: said(&answer),
            }),
            other => bail!("recording the trade #{}: austeris answered {other}: {}", trade.legacy_id, said(&answer)),
        }
    }
    Ok(())
}

/// Takes the snapshots, each under its key, and reads what each says.
async fn snapshots(api: &Api, plan: &Plan, written: &mut Written) -> Result<()> {
    eprintln!("Taking {} snapshots", plan.snapshots.len());
    for snapshot in &plan.snapshots {
        let (status, answer) = api
            .call(
                Method::POST,
                "/api/v1/portfolio/snapshots",
                Some(&json!({ "on": snapshot.day, "currency": "USD", "note": snapshot.note, "idempotency_key": snapshot.key })),
            )
            .await?;
        match status {
            StatusCode::CREATED => {}
            StatusCode::OK => written.snapshots_already += 1,
            other => bail!("taking the snapshot of {}: austeris answered {other}: {}", snapshot.day, said(&answer)),
        }
        written.snapshots.push(Books::of(answer, &written.accounts)?);
    }
    Ok(())
}

/// The account a wallet goes into: one of its name the import can use, or a
/// new one - named as the wallet was, or with `(2025)` after it when that
/// name is taken by an account of another kind.
async fn place(api: &Api, existing: &[Account], account: &AccountPlan) -> Result<Placed> {
    for name in [account.name.clone(), format!("{} (2025)", account.name)] {
        let found = existing.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(&name));
        match found {
            Some(found) if found.fits() => {
                return Ok(Placed {
                    wallet_id: account.wallet_id,
                    id: found.id.clone(),
                    name: found.name.clone(),
                    created: false,
                    opening: found.opening_balance.normalize(),
                });
            }
            Some(_) => {}
            None => {
                let made = api
                    .ok(
                        Method::POST,
                        "/api/v1/ledger/accounts",
                        Some(&json!({ "kind": "crypto_wallet", "name": name, "currency": MONEY, "opening_balance": account.opening.to_string() })),
                    )
                    .await?;
                let made: Account = serde_json::from_value(made).context("reading the account made")?;
                return Ok(Placed {
                    wallet_id: account.wallet_id,
                    id: made.id,
                    name: made.name,
                    created: true,
                    opening: made.opening_balance.normalize(),
                });
            }
        }
    }
    bail!(
        "accounts called {0} and {0} (2025) are there, and neither is a wallet's account in {MONEY}; rename one, and import again",
        account.name
    )
}
