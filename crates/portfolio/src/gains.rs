//! What sales gained, and the lots behind every position.
//!
//! A sale's gain is what it brought in, less its fee, beside what the units it
//! took cost - the lots its account's method picked (ADR 0017). Told in one
//! currency: a sale's proceeds at the rate of its day, and each lot it took
//! at the rate of the day that lot was acquired, which is how the money that
//! bought it is counted where tax is paid in another currency than the trade.

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::lots::{self, Lot};
use crate::model::{ClosedLot, Gain, Gains, OpenLot, PositionLots};
use crate::repository;
use crate::services::{Books, Market};
use crate::value::{Rates, Unvalued};

/// What the sales of a period - its first and last days included - gained,
/// in `currency`.
///
/// # Errors
///
/// Returns [`Unvalued`] when the ledger or the market does not answer, or the
/// trades cannot be read.
pub async fn gains<B: Books, M: Market>(
    pool: &PgPool,
    books: &B,
    market: &M,
    owner: Uuid,
    from: NaiveDate,
    to: NaiveDate,
    currency: &str,
) -> Result<Gains, Unvalued> {
    // A sale takes from what was held before it; nothing after the period
    // changes what a sale in it took.
    let trades = repository::all_trades(pool, owner, Some(to)).await?;
    let book = lots::work_out(&trades, &repository::cost_methods(pool, owner).await?);
    let sales: Vec<_> = book.disposals.into_iter().filter(|sale| (from..=to).contains(&sale.occurred_on)).collect();

    let names: HashMap<Uuid, String> = books
        .accounts(owner)
        .await
        .map_err(Unvalued::Ledger)?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let symbols = symbols(market, sales.iter().map(|sale| sale.instrument_id)).await?;

    let mut rates = Rates::default();
    let mut unvalued: Vec<String> = Vec::new();
    let (mut proceeds, mut cost) = (Decimal::ZERO, Decimal::ZERO);
    let mut out = Vec::with_capacity(sales.len());
    for sale in sales {
        let brought = rates.convert(books, sale.proceeds, &sale.currency, currency, sale.occurred_on).await?;
        if brought.is_none() {
            note(&mut unvalued, &sale.currency);
        }
        let mut took = Some(Decimal::ZERO);
        for lot in &sale.lots {
            let converted = rates.convert(books, lot.cost, &lot.currency, currency, lot.acquired_on).await?;
            if converted.is_none() {
                note(&mut unvalued, &lot.currency);
            }
            took = took.zip(converted).map(|(sum, more)| sum + more);
        }
        let (brought, took) = (brought.map(|money| round(money, currency)), took.map(|money| round(money, currency)));
        let gain = brought.zip(took).map(|(brought, took)| brought - took);
        if let (Some(brought), Some(took)) = (brought, took) {
            proceeds += brought;
            cost += took;
        }
        out.push(Gain {
            trade_id: sale.trade_id,
            account_id: sale.account_id,
            account_name: names.get(&sale.account_id).cloned().unwrap_or_default(),
            instrument_id: sale.instrument_id,
            symbol: symbols.get(&sale.instrument_id).cloned().unwrap_or_else(|| sale.instrument_id.to_string()),
            occurred_at: sale.occurred_at,
            occurred_on: sale.occurred_on,
            method: sale.method,
            quantity: sale.quantity.normalize(),
            proceeds: brought,
            cost: took,
            gain: gain.map(|gain| gain.normalize()),
            lots: sale.lots.iter().map(closed).collect(),
        });
    }
    out.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at).then_with(|| b.trade_id.cmp(&a.trade_id)));
    unvalued.sort();

    Ok(Gains {
        currency: currency.to_owned(),
        from,
        to,
        sales: out,
        proceeds: proceeds.normalize(),
        cost: cost.normalize(),
        gain: (proceeds - cost).normalize(),
        unvalued,
    })
}

/// Every position and the lots it is made of - now, or as of the end of a
/// day - in the currency each lot was acquired in.
///
/// # Errors
///
/// Returns [`Unvalued`] when the market does not answer, or the trades cannot
/// be read.
pub async fn open_lots<M: Market>(pool: &PgPool, market: &M, owner: Uuid, on: Option<NaiveDate>) -> Result<Vec<PositionLots>, Unvalued> {
    let trades = repository::all_trades(pool, owner, on).await?;
    let methods = repository::cost_methods(pool, owner).await?;
    let book = lots::work_out(&trades, &methods);
    let symbols = symbols(market, book.open.iter().map(|lot| lot.instrument_id)).await?;

    let mut positions: Vec<PositionLots> = Vec::new();
    for lot in &book.open {
        let start = positions
            .last()
            .is_none_or(|position| (position.account_id, position.instrument_id) != (lot.account_id, lot.instrument_id));
        if start {
            positions.push(PositionLots {
                account_id: lot.account_id,
                instrument_id: lot.instrument_id,
                symbol: symbols.get(&lot.instrument_id).cloned().unwrap_or_else(|| lot.instrument_id.to_string()),
                method: methods.get(&lot.account_id).copied().unwrap_or_default(),
                quantity: Decimal::ZERO,
                lots: Vec::new(),
            });
        }
        let Some(position) = positions.last_mut() else { continue };
        position.quantity += lot.quantity;
        position.lots.push(OpenLot {
            trade_id: lot.trade_id,
            acquired_at: lot.acquired_at,
            acquired_on: lot.acquired_on,
            quantity: lot.quantity.normalize(),
            cost: round(lot.cost, &lot.currency),
            unit_cost: (lot.cost / lot.quantity).round_dp(18).normalize(),
            currency: lot.currency.clone(),
        });
    }
    for position in &mut positions {
        position.quantity = position.quantity.normalize();
    }
    Ok(positions)
}

/// A part of a lot a sale took, as it is answered.
fn closed(lot: &Lot) -> ClosedLot {
    ClosedLot {
        trade_id: lot.trade_id,
        acquired_at: lot.acquired_at,
        acquired_on: lot.acquired_on,
        quantity: lot.quantity.normalize(),
        cost: round(lot.cost, &lot.currency),
        currency: lot.currency.clone(),
    }
}

/// Money written to the digits of its currency.
fn round(amount: Decimal, currency: &str) -> Decimal {
    austeris_ledger::currency::round(amount, currency).normalize()
}

/// Names a currency once among those a report could not convert.
fn note(unvalued: &mut Vec<String>, currency: &str) {
    if !unvalued.iter().any(|seen| seen == currency) {
        unvalued.push(currency.to_owned());
    }
}

/// The tickers of these instruments, as the market knows them.
async fn symbols<M: Market>(market: &M, ids: impl Iterator<Item = Uuid>) -> Result<HashMap<Uuid, String>, Unvalued> {
    let mut ids: Vec<Uuid> = ids.collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(market
        .instruments(&ids)
        .await
        .map_err(Unvalued::Market)?
        .into_iter()
        .map(|instrument| (instrument.id, instrument.symbol))
        .collect())
}
