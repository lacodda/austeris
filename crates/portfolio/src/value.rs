//! What the portfolio is worth: each position at the market's price, and the
//! money beside it on the accounts it is held on, in one currency.
//!
//! A stablecoin on an exchange is money there, so it is the account's balance
//! in the ledger and is valued at the ledger's rate - not a position priced by
//! the market (ADR 0015). What has no price or rate in the currency asked for
//! is shown with its quantity and left out of the total, by name: a total that
//! silently drops it looks like an answer.
//!
//! Beside what a position is worth, what it cost: its lots (ADR 0017), each in
//! the currency asked for at the rate of the day it was acquired - what the
//! money that bought it was worth then - and what it has gained since.

use std::collections::HashMap;

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::Status;
use uuid::Uuid;

use crate::lots::{self, Lot};
use crate::model::{AccountValue, Holding, LineKind, Valuation};
use crate::repository;
use crate::services::{Books, Market, Price};

/// Why a valuation could not be made.
#[derive(Debug, thiserror::Error)]
pub enum Unvalued {
    /// The ledger did not answer.
    #[error("the ledger is not answering: {0}")]
    Ledger(Status),
    /// The market did not answer.
    #[error("the market is not answering: {0}")]
    Market(Status),
    /// This service's own schema could not be read.
    #[error(transparent)]
    Here(#[from] anyhow::Error),
}

/// What the portfolio holds now - or held on the day `on`, a day past - and
/// what it was worth in `currency`.
///
/// Now is every trade, at the latest prices and today's money. A day past is
/// the trades of that day and before, at the prices observed by its end and
/// the money the accounts held then, at that day's rates: the books know days,
/// not moments, so a day is what can be shown truthfully.
///
/// # Errors
///
/// Returns [`Unvalued`] when the ledger or the market does not answer, or the
/// trades cannot be read.
pub async fn valuation<B: Books, M: Market>(
    pool: &PgPool,
    books: &B,
    market: &M,
    owner: Uuid,
    on: Option<NaiveDate>,
    currency: &str,
) -> Result<Valuation, Unvalued> {
    let now = Utc::now();
    // A day's prices are the last observed before it ended.
    let moment = on.map_or(now, |day| {
        (day + Days::new(1)).and_time(NaiveTime::MIN).and_utc() - chrono::Duration::seconds(1)
    });
    let accounts = books.accounts(owner).await.map_err(Unvalued::Ledger)?;
    let positions = repository::positions(pool, owner, on).await?;
    let trades = repository::all_trades(pool, owner, on).await?;
    let book = lots::work_out(&trades, &repository::cost_methods(pool, owner).await?);

    let mut ids: Vec<Uuid> = positions.iter().map(|position| position.instrument_id).collect();
    ids.sort_unstable();
    ids.dedup();
    let instruments: HashMap<Uuid, _> = market
        .instruments(&ids)
        .await
        .map_err(Unvalued::Market)?
        .into_iter()
        .map(|instrument| (instrument.id, instrument))
        .collect();
    // Now is every trade, and the money they moved is booked under the day
    // each happened where the person was - a day ahead of UTC's, east of it.
    // The money beside them is the books' as of the latest of those days, so
    // a trade made this morning in Moscow is not counted twice: once in the
    // coins it bought, and again in the money not yet gone.
    let day = on.unwrap_or_else(|| {
        let today = now.date_naive();
        trades.iter().map(|trade| trade.occurred_on).max().map_or(today, |latest| latest.max(today))
    });
    let prices = prices_in(books, market, &ids, currency, on.map(|_| moment), day).await?;
    let costs = costs_in(books, &book.open, currency).await?;
    let cash: HashMap<Uuid, _> = books
        .cash(owner, day, currency)
        .await
        .map_err(Unvalued::Ledger)?
        .into_iter()
        .map(|cash| (cash.account_id, cash))
        .collect();

    let mut valued = Vec::new();
    for account in accounts.iter().filter(|account| account.holds_positions()) {
        let mut holdings = Vec::new();
        if let Some(money) = cash.get(&account.id).filter(|money| !money.amount.is_zero()) {
            // The ledger converts; a balance already in the currency asked
            // for is worth itself.
            let same = account.currency == currency;
            let value = if same { Some(money.amount) } else { money.converted };
            holdings.push(Holding {
                kind: LineKind::Cash,
                instrument_id: None,
                symbol: account.currency.clone(),
                name: String::new(),
                quantity: money.amount.normalize(),
                price: value.map(|value| (value / money.amount).normalize()),
                priced_at: if same {
                    None
                } else {
                    money.rate_on.map(|day| day.and_time(NaiveTime::MIN).and_utc())
                },
                value: value.map(|value| value.normalize()),
                stale: !same && money.stale,
                cost: None,
                gain: None,
            });
        }
        for position in positions.iter().filter(|position| position.account_id == account.id) {
            let instrument = instruments.get(&position.instrument_id);
            let price = prices.get(&position.instrument_id);
            holdings.push(Holding {
                kind: LineKind::Position,
                instrument_id: Some(position.instrument_id),
                // An instrument the market no longer knows is still held; it is
                // named by its id rather than dropped.
                symbol: instrument.map_or_else(|| position.instrument_id.to_string(), |instrument| instrument.symbol.clone()),
                name: instrument.map(|instrument| instrument.name.clone()).unwrap_or_default(),
                quantity: position.quantity.normalize(),
                price: price.map(|price| price.price.normalize()),
                priced_at: price.map(|price| price.observed_at),
                value: price.map(|price| (position.quantity * price.price).normalize()),
                stale: price.is_some_and(|price| price.stale),
                cost: costs.get(&(account.id, position.instrument_id)).copied().flatten(),
                gain: None,
            });
        }
        // A closed account with nothing on it is history, not a holding.
        if holdings.is_empty() && account.closed {
            continue;
        }
        valued.push(AccountValue {
            account_id: account.id,
            name: account.name.clone(),
            holdings,
            total: Decimal::ZERO,
        });
    }
    Ok(assemble(day, moment, currency.to_owned(), valued))
}

/// The currencies a price is looked for in when there is none in the one
/// asked for, in order: the ones price sources quote in.
const REFERENCE_CURRENCIES: [&str; 2] = ["USD", "EUR"];

/// What a unit of each instrument is worth in `currency`: the latest price, or
/// the last observed by `by` - the end of `day`.
///
/// A source quotes in its own currency - a coin in dollars - and a person
/// reads their portfolio in theirs. What has no price in it is priced in a
/// currency sources do quote in, at the ledger's rate for the day: the one
/// place money changes currency (ADR 0009). Either being stale makes the
/// price stale.
pub(crate) async fn prices_in<B: Books, M: Market>(
    books: &B,
    market: &M,
    ids: &[Uuid],
    currency: &str,
    by: Option<DateTime<Utc>>,
    day: NaiveDate,
) -> Result<HashMap<Uuid, Price>, Unvalued> {
    let mut prices: HashMap<Uuid, Price> = priced(market, ids, currency, by)
        .await?
        .into_iter()
        .map(|price| (price.instrument_id, price))
        .collect();
    for reference in REFERENCE_CURRENCIES.into_iter().filter(|reference| *reference != currency) {
        let missing: Vec<Uuid> = ids.iter().copied().filter(|id| !prices.contains_key(id)).collect();
        if missing.is_empty() {
            break;
        }
        let found = priced(market, &missing, reference, by).await?;
        if found.is_empty() {
            continue;
        }
        let Some(rate) = books.rate(reference, currency, day).await.map_err(Unvalued::Ledger)? else {
            continue;
        };
        for price in found {
            prices.insert(
                price.instrument_id,
                Price {
                    price: price.price * rate.rate,
                    stale: price.stale || rate.stale,
                    ..price
                },
            );
        }
    }
    Ok(prices)
}

/// What the open lots of each position cost in `currency`: each lot at the
/// ledger's rate of the day it was acquired. `None` for a position one of
/// whose lots is in a currency with no rate into it on that day.
async fn costs_in<B: Books>(books: &B, open: &[Lot], currency: &str) -> Result<HashMap<(Uuid, Uuid), Option<Decimal>>, Unvalued> {
    let mut rates = Rates::default();
    let mut costs: HashMap<(Uuid, Uuid), Option<Decimal>> = HashMap::new();
    for lot in open {
        let converted = rates.convert(books, lot.cost, &lot.currency, currency, lot.acquired_on).await?;
        let cost = costs.entry((lot.account_id, lot.instrument_id)).or_insert(Some(Decimal::ZERO));
        *cost = match (*cost, converted) {
            (Some(sum), Some(more)) => Some(sum + more),
            _ => None,
        };
    }
    Ok(costs
        .into_iter()
        .map(|(position, cost)| (position, cost.map(|cost| currency_round(cost, currency))))
        .collect())
}

/// A cost or a gain written to the digits of the currency it is in - the
/// ones it has, or all of them for a currency nobody standardised.
fn currency_round(amount: Decimal, currency: &str) -> Decimal {
    austeris_ledger::currency::round(amount, currency).normalize()
}

/// The ledger's rates, each asked for once: a report of a year's sales asks
/// for the same day's rate many times over.
#[derive(Default)]
pub(crate) struct Rates {
    known: HashMap<(String, String, NaiveDate), Option<Decimal>>,
}

impl Rates {
    /// `amount` of `from` in `to` at the rate in force on `day`; `None` when
    /// the ledger knows none.
    pub(crate) async fn convert<B: Books>(&mut self, books: &B, amount: Decimal, from: &str, to: &str, day: NaiveDate) -> Result<Option<Decimal>, Unvalued> {
        if from == to || amount.is_zero() {
            return Ok(Some(amount));
        }
        let key = (from.to_owned(), to.to_owned(), day);
        let rate = if let Some(rate) = self.known.get(&key) {
            *rate
        } else {
            let rate = books.rate(from, to, day).await.map_err(Unvalued::Ledger)?.map(|rate| rate.rate);
            self.known.insert(key, rate);
            rate
        };
        Ok(rate.map(|rate| amount * rate))
    }
}

/// The prices of these instruments in `currency`: the latest, or the last
/// observed by `by`.
async fn priced<M: Market>(market: &M, ids: &[Uuid], currency: &str, by: Option<DateTime<Utc>>) -> Result<Vec<Price>, Unvalued> {
    let Some(moment) = by else {
        return market.prices(ids, currency).await.map_err(Unvalued::Market);
    };
    let mut found = Vec::new();
    for id in ids {
        if let Some(price) = market.price_at(*id, currency, moment).await.map_err(Unvalued::Market)? {
            found.push(price);
        }
    }
    Ok(found)
}

/// A valuation of these accounts: each in order, its money first and its
/// positions by ticker, and what everything with a value adds up to.
#[must_use]
pub fn assemble(on: NaiveDate, at: DateTime<Utc>, currency: String, mut accounts: Vec<AccountValue>) -> Valuation {
    let mut unvalued: Vec<String> = Vec::new();
    let mut stale = false;
    let mut total = Decimal::ZERO;
    let mut gain = Some(Decimal::ZERO);
    let mut valued_positions = 0_usize;
    for account in &mut accounts {
        account.holdings.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.symbol.cmp(&b.symbol))
                .then_with(|| a.instrument_id.cmp(&b.instrument_id))
        });
        account.total = account.holdings.iter().filter_map(|holding| holding.value).sum::<Decimal>().normalize();
        for holding in &mut account.holdings {
            holding.gain = match (holding.value, holding.cost) {
                (Some(value), Some(cost)) if holding.kind == LineKind::Position => Some((value - cost).normalize()),
                _ => None,
            };
            match holding.value {
                Some(_) => stale |= holding.stale,
                None if !unvalued.contains(&holding.symbol) => unvalued.push(holding.symbol.clone()),
                None => {}
            }
            // A position in the total whose cost is not known leaves the
            // total's gain unknown, rather than smaller than it is.
            if holding.kind == LineKind::Position && holding.value.is_some() {
                valued_positions += 1;
                gain = match (gain, holding.gain) {
                    (Some(sum), Some(more)) => Some(sum + more),
                    _ => None,
                };
            }
        }
        total += account.total;
    }
    accounts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.account_id.cmp(&b.account_id)));
    unvalued.sort();
    Valuation {
        on,
        at,
        currency,
        accounts,
        total: total.normalize(),
        unvalued,
        stale,
        gain: if valued_positions == 0 { None } else { gain.map(|gain| gain.normalize()) },
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::Utc;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::assemble;
    use crate::model::{AccountValue, Holding, LineKind};

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    fn holding(kind: LineKind, symbol: &str, value: Option<&str>, stale: bool) -> Holding {
        Holding {
            kind,
            instrument_id: (kind == LineKind::Position).then(Uuid::new_v4),
            symbol: symbol.to_owned(),
            name: String::new(),
            quantity: Decimal::ONE,
            price: value.map(decimal),
            priced_at: None,
            value: value.map(decimal),
            stale,
            cost: None,
            gain: None,
        }
    }

    #[test]
    fn what_has_no_price_is_named_and_left_out_of_the_total() {
        let account = AccountValue {
            account_id: Uuid::new_v4(),
            name: "Exchange".to_owned(),
            holdings: vec![
                holding(LineKind::Position, "ETH", None, false),
                holding(LineKind::Position, "BTC", Some("600.5"), false),
                holding(LineKind::Cash, "USDT", Some("399.5"), false),
            ],
            total: Decimal::ZERO,
        };
        let valuation = assemble(Utc::now().date_naive(), Utc::now(), "USD".to_owned(), vec![account]);
        assert_eq!(valuation.total, decimal("1000"));
        assert_eq!(valuation.unvalued, ["ETH"]);
        assert!(!valuation.stale);
        let order: Vec<&str> = valuation.accounts[0].holdings.iter().map(|holding| holding.symbol.as_str()).collect();
        assert_eq!(order, ["USDT", "BTC", "ETH"], "money first, then positions by ticker");
    }

    #[test]
    fn a_gain_is_the_value_less_the_cost_and_the_total_has_one_only_when_every_position_does() {
        let mut btc = holding(LineKind::Position, "BTC", Some("650"), false);
        btc.cost = Some(decimal("601"));
        let mut eth = holding(LineKind::Position, "ETH", Some("100"), false);
        eth.cost = Some(decimal("120"));
        let cash = holding(LineKind::Cash, "USDT", Some("50"), false);
        let account = AccountValue {
            account_id: Uuid::new_v4(),
            name: "Exchange".to_owned(),
            holdings: vec![btc, eth, cash],
            total: Decimal::ZERO,
        };
        let valuation = assemble(Utc::now().date_naive(), Utc::now(), "USDT".to_owned(), vec![account.clone()]);
        let gains: Vec<Option<Decimal>> = valuation.accounts[0].holdings.iter().map(|holding| holding.gain).collect();
        assert_eq!(gains, [None, Some(decimal("49")), Some(decimal("-20"))], "money first, then BTC and ETH");
        assert_eq!(valuation.gain, Some(decimal("29")));

        let mut uncosted = account;
        uncosted.holdings[1].cost = None;
        assert_eq!(assemble(Utc::now().date_naive(), Utc::now(), "USDT".to_owned(), vec![uncosted]).gain, None);
    }

    #[test]
    fn a_stale_price_in_the_total_says_so_and_one_left_out_does_not() {
        let account = AccountValue {
            account_id: Uuid::new_v4(),
            name: "Broker".to_owned(),
            holdings: vec![holding(LineKind::Position, "DWLD", Some("100"), true)],
            total: Decimal::ZERO,
        };
        assert!(assemble(Utc::now().date_naive(), Utc::now(), "EUR".to_owned(), vec![account.clone()]).stale);

        let mut unpriced = account;
        unpriced.holdings[0].value = None;
        unpriced.holdings[0].price = None;
        assert!(!assemble(Utc::now().date_naive(), Utc::now(), "EUR".to_owned(), vec![unpriced]).stale);
    }
}
