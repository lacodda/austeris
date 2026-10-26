//! What the portfolio is worth: each position at the market's price, and the
//! money beside it on the accounts it is held on, in one currency.
//!
//! A stablecoin on an exchange is money there, so it is the account's balance
//! in the ledger and is valued at the ledger's rate - not a position priced by
//! the market (ADR 0015). What has no price or rate in the currency asked for
//! is shown with its quantity and left out of the total, by name: a total that
//! silently drops it looks like an answer.

use std::collections::HashMap;

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::Status;
use uuid::Uuid;

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
    let prices: HashMap<Uuid, Price> = if on.is_none() {
        market.prices(&ids, currency).await.map_err(Unvalued::Market)?
    } else {
        let mut found = Vec::new();
        for id in &ids {
            if let Some(price) = market.price_at(*id, currency, moment).await.map_err(Unvalued::Market)? {
                found.push(price);
            }
        }
        found
    }
    .into_iter()
    .map(|price| (price.instrument_id, price))
    .collect();
    let day = on.unwrap_or_else(|| now.date_naive());
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

/// A valuation of these accounts: each in order, its money first and its
/// positions by ticker, and what everything with a value adds up to.
#[must_use]
pub fn assemble(on: NaiveDate, at: DateTime<Utc>, currency: String, mut accounts: Vec<AccountValue>) -> Valuation {
    let mut unvalued: Vec<String> = Vec::new();
    let mut stale = false;
    let mut total = Decimal::ZERO;
    for account in &mut accounts {
        account.holdings.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.symbol.cmp(&b.symbol))
                .then_with(|| a.instrument_id.cmp(&b.instrument_id))
        });
        account.total = account.holdings.iter().filter_map(|holding| holding.value).sum::<Decimal>().normalize();
        for holding in &account.holdings {
            match holding.value {
                Some(_) => stale |= holding.stale,
                None if !unvalued.contains(&holding.symbol) => unvalued.push(holding.symbol.clone()),
                None => {}
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
