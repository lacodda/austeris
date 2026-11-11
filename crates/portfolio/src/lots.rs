//! Lots: what each acquisition of units cost, and which of them a sale or a
//! transfer took.
//!
//! Every buy and every receive is a lot - so many units, at a moment, for so
//! much in the account's currency, the buy's fee included, as a cost basis
//! counts it. A sale takes units out of the lots of its account, and which
//! ones is the account's [`Method`]: the oldest first, the newest first, or a
//! share of every lot - which is what an average cost is. What a sale took
//! is what its units cost, and its gain is what it brought in less that.
//!
//! A transfer is not a sale: the units it takes keep the moment they were
//! acquired and what they cost, and arrive as lots on the other account. What
//! the network took is units lost on the way, so the units that arrive cost
//! what all of those that left did.
//!
//! Worked out from the trades every time, as positions are, and never stored
//! beside them (ADR 0017): a correction to an old trade, or a change of method,
//! is in every answer from then on.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{Trade, TradeKind};
use crate::trade::money;

/// How an account's sales and transfers out pick the lots they take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "cost_method", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// First in, first out: the oldest lot first.
    #[default]
    Fifo,
    /// Last in, first out: the newest lot first.
    Lifo,
    /// A share of every lot held, by its size: each unit costs the average.
    Average,
}

/// Units acquired together, or what is left of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lot {
    /// The buy or the receive they came from.
    pub trade_id: Uuid,
    /// The account they are on now.
    pub account_id: Uuid,
    /// What they are.
    pub instrument_id: Uuid,
    /// When they were acquired - kept across a transfer.
    pub acquired_at: DateTime<Utc>,
    /// The day they were acquired, where the person was.
    pub acquired_on: NaiveDate,
    /// How many.
    pub quantity: Decimal,
    /// What they cost, in `currency`: the price and the buy's fee, or what a
    /// reward was worth when it arrived.
    pub cost: Decimal,
    /// The currency of the account they were acquired on.
    pub currency: String,
}

/// A sale, and the lots it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disposal {
    /// The sale.
    pub trade_id: Uuid,
    /// Where it was made.
    pub account_id: Uuid,
    /// What was sold.
    pub instrument_id: Uuid,
    /// When.
    pub occurred_at: DateTime<Utc>,
    /// The day its money is booked under.
    pub occurred_on: NaiveDate,
    /// How the account picked the lots.
    pub method: Method,
    /// How many units.
    pub quantity: Decimal,
    /// What it brought in, less its fee, in `currency`.
    pub proceeds: Decimal,
    /// The sale's currency: the account's.
    pub currency: String,
    /// The lots it took - how many units of each, and what those cost.
    pub lots: Vec<Lot>,
}

/// What a person's trades come to: the lots still held, and every sale with
/// the lots it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Book {
    /// What is held, by account and instrument, the oldest lot first.
    pub open: Vec<Lot>,
    /// Every sale, in the order they happened.
    pub disposals: Vec<Disposal>,
}

/// Where a trade falls among the others at the same instant: what arrives
/// before what moves, before what leaves - as the schema adds a position up.
fn rank(kind: TradeKind) -> u8 {
    match kind {
        TradeKind::Buy | TradeKind::Receive => 0,
        TradeKind::Transfer => 1,
        TradeKind::Sell => 2,
    }
}

/// Works out the lots of a person's trades, each account counting by its
/// method - first in, first out where none is set.
#[must_use]
pub fn work_out<S: std::hash::BuildHasher>(trades: &[Trade], methods: &HashMap<Uuid, Method, S>) -> Book {
    let mut ordered: Vec<&Trade> = trades.iter().collect();
    ordered.sort_by(|a, b| {
        a.occurred_at
            .cmp(&b.occurred_at)
            .then_with(|| rank(a.kind).cmp(&rank(b.kind)))
            .then_with(|| a.id.cmp(&b.id))
    });

    let method = |account: Uuid| methods.get(&account).copied().unwrap_or_default();
    let mut held: HashMap<(Uuid, Uuid), Vec<Lot>> = HashMap::new();
    let mut disposals = Vec::new();
    for trade in ordered {
        let currency = trade.currency.clone().unwrap_or_default();
        match trade.kind {
            TradeKind::Buy | TradeKind::Receive => {
                let cost = match trade.kind {
                    // What left the account, as the books have it.
                    TradeKind::Buy => -money(trade.kind, trade.quantity, trade.price, trade.fee, trade.currency.as_deref()).unwrap_or_default(),
                    // What it was worth when it arrived, rounded as the
                    // account's money is.
                    _ => austeris_ledger::currency::round(trade.quantity * trade.price.unwrap_or_default(), &currency),
                };
                place(
                    &mut held,
                    Lot {
                        trade_id: trade.id,
                        account_id: trade.account_id,
                        instrument_id: trade.instrument_id,
                        acquired_at: trade.occurred_at,
                        acquired_on: trade.occurred_on,
                        quantity: trade.quantity,
                        cost,
                        currency,
                    },
                );
            }
            TradeKind::Transfer => {
                let Some(to) = trade.to_account_id else { continue };
                let taken = take(&mut held, trade, method(trade.account_id));
                for lot in arrived(taken, trade.quantity - trade.fee) {
                    place(&mut held, Lot { account_id: to, ..lot });
                }
            }
            TradeKind::Sell => {
                let account_method = method(trade.account_id);
                let lots = take(&mut held, trade, account_method);
                disposals.push(Disposal {
                    trade_id: trade.id,
                    account_id: trade.account_id,
                    instrument_id: trade.instrument_id,
                    occurred_at: trade.occurred_at,
                    occurred_on: trade.occurred_on,
                    method: account_method,
                    quantity: trade.quantity,
                    proceeds: money(trade.kind, trade.quantity, trade.price, trade.fee, trade.currency.as_deref()).unwrap_or_default(),
                    currency,
                    lots,
                });
            }
        }
    }

    let mut open: Vec<Lot> = held.into_values().flatten().filter(|lot| !lot.quantity.is_zero()).collect();
    open.sort_by_key(|lot| (lot.account_id, lot.instrument_id, lot.acquired_at, lot.trade_id));
    Book { open, disposals }
}

/// Puts a lot among those of its position, in the order they were acquired.
fn place(held: &mut HashMap<(Uuid, Uuid), Vec<Lot>>, lot: Lot) {
    let lots = held.entry((lot.account_id, lot.instrument_id)).or_default();
    let at = lots.partition_point(|other| (other.acquired_at, other.trade_id) <= (lot.acquired_at, lot.trade_id));
    lots.insert(at, lot);
}

/// Takes a trade's units out of the lots of the account it left, as the
/// method picks them: what of each lot it took, and what that part cost.
///
/// The schema refuses a position below nothing, so the units are there. Should
/// they not be - two transfers chained at one instant, added up in another
/// order - what is missing is taken at no cost, from the moment it left,
/// rather than lost.
fn take(held: &mut HashMap<(Uuid, Uuid), Vec<Lot>>, trade: &Trade, method: Method) -> Vec<Lot> {
    let lots = held.entry((trade.account_id, trade.instrument_id)).or_default();
    let wanted = trade.quantity;
    let mut taken = Vec::new();
    match method {
        Method::Fifo | Method::Lifo => {
            let mut left = wanted;
            while left > Decimal::ZERO && !lots.is_empty() {
                let index = if method == Method::Fifo { 0 } else { lots.len() - 1 };
                let piece = split(&mut lots[index], left);
                left -= piece.quantity;
                taken.push(piece);
                if lots[index].quantity.is_zero() {
                    lots.remove(index);
                }
            }
        }
        Method::Average => {
            let total: Decimal = lots.iter().map(|lot| lot.quantity).sum();
            if wanted >= total {
                taken.append(lots);
            } else if !total.is_zero() {
                let count = lots.len();
                let mut given = Decimal::ZERO;
                for (index, lot) in lots.iter_mut().enumerate() {
                    // The last share is what is left to take, so the shares
                    // add up to the units sold however the division rounds.
                    let share = if index + 1 == count { wanted - given } else { lot.quantity * wanted / total };
                    let piece = split(lot, share.min(lot.quantity));
                    given += piece.quantity;
                    taken.push(piece);
                }
                lots.retain(|lot| !lot.quantity.is_zero());
            }
        }
    }
    let got: Decimal = taken.iter().map(|lot| lot.quantity).sum();
    if got < wanted {
        taken.push(Lot {
            trade_id: trade.id,
            account_id: trade.account_id,
            instrument_id: trade.instrument_id,
            acquired_at: trade.occurred_at,
            acquired_on: trade.occurred_on,
            quantity: wanted - got,
            cost: Decimal::ZERO,
            currency: trade.currency.clone().unwrap_or_default(),
        });
    }
    taken
}

/// Splits up to `quantity` units off a lot, with their share of its cost; the
/// lot keeps the rest. All of a lot takes all of its cost, to the last digit.
fn split(lot: &mut Lot, quantity: Decimal) -> Lot {
    let quantity = quantity.min(lot.quantity);
    let cost = if quantity == lot.quantity {
        lot.cost
    } else {
        lot.cost * quantity / lot.quantity
    };
    lot.quantity -= quantity;
    lot.cost -= cost;
    Lot { quantity, cost, ..lot.clone() }
}

/// The lots a transfer took, as they arrive: fewer units by what the network
/// took, each lot by its share, and each still costing what it did.
fn arrived(mut taken: Vec<Lot>, arriving: Decimal) -> Vec<Lot> {
    let sent: Decimal = taken.iter().map(|lot| lot.quantity).sum();
    if sent.is_zero() || sent == arriving {
        return taken;
    }
    let count = taken.len();
    let mut given = Decimal::ZERO;
    for (index, lot) in taken.iter_mut().enumerate() {
        lot.quantity = if index + 1 == count {
            arriving - given
        } else {
            lot.quantity * arriving / sent
        };
        given += lot.quantity;
    }
    taken.retain(|lot| lot.quantity > Decimal::ZERO);
    taken
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use chrono::{DateTime, Duration, TimeZone, Utc};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{Method, work_out};
    use crate::model::{Trade, TradeKind};

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    fn at(hours: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 5, 12, 0, 0).single().expect("a moment") + Duration::hours(hours)
    }

    fn trade(kind: TradeKind, account: Uuid, quantity: &str, price: Option<&str>, fee: &str, hours: i64) -> Trade {
        let when = at(hours);
        Trade {
            id: Uuid::new_v4(),
            kind,
            account_id: account,
            to_account_id: None,
            instrument_id: Uuid::nil(),
            quantity: decimal(quantity),
            price: price.map(decimal),
            currency: (kind != TradeKind::Transfer).then(|| "USDT".to_owned()),
            fee: decimal(fee),
            occurred_at: when,
            occurred_on: when.date_naive(),
            note: String::new(),
            entry_id: None,
            money: None,
            idempotency_key: None,
            created_at: when,
            updated_at: when,
        }
    }

    /// Two buys at different prices and a sale of part of them.
    fn three(account: Uuid) -> Vec<Trade> {
        vec![
            trade(TradeKind::Buy, account, "1", Some("100"), "1", 0),
            trade(TradeKind::Buy, account, "1", Some("200"), "2", 1),
            trade(TradeKind::Sell, account, "1.5", Some("300"), "3", 2),
        ]
    }

    fn methods(account: Uuid, method: Method) -> HashMap<Uuid, Method> {
        HashMap::from([(account, method)])
    }

    #[test]
    fn first_in_first_out_sells_the_oldest_lot_first() {
        let account = Uuid::new_v4();
        let book = work_out(&three(account), &methods(account, Method::Fifo));
        let sale = &book.disposals[0];
        // All of the first lot (101 with its fee) and half of the second (202 / 2).
        assert_eq!(sale.lots.iter().map(|lot| lot.cost).sum::<Decimal>(), decimal("202"));
        assert_eq!(sale.proceeds, decimal("447"));
        assert_eq!(book.open.len(), 1);
        assert_eq!((book.open[0].quantity, book.open[0].cost), (decimal("0.5"), decimal("101")));
    }

    #[test]
    fn last_in_first_out_sells_the_newest_lot_first() {
        let account = Uuid::new_v4();
        let book = work_out(&three(account), &methods(account, Method::Lifo));
        // All of the second lot (202) and half of the first (101 / 2).
        assert_eq!(book.disposals[0].lots.iter().map(|lot| lot.cost).sum::<Decimal>(), decimal("252.5"));
        assert_eq!((book.open[0].quantity, book.open[0].cost), (decimal("0.5"), decimal("50.5")));
    }

    #[test]
    fn an_average_takes_a_share_of_every_lot_and_each_unit_costs_the_same() {
        let account = Uuid::new_v4();
        let book = work_out(&three(account), &methods(account, Method::Average));
        // 303 for 2 units: 1.5 of them cost 227.25, whichever lots they are.
        assert_eq!(book.disposals[0].lots.iter().map(|lot| lot.cost).sum::<Decimal>(), decimal("227.25"));
        assert_eq!(book.disposals[0].lots.iter().map(|lot| lot.quantity).sum::<Decimal>(), decimal("1.5"));
        let left: Decimal = book.open.iter().map(|lot| lot.quantity).sum();
        let cost: Decimal = book.open.iter().map(|lot| lot.cost).sum();
        assert_eq!((left, cost), (decimal("0.5"), decimal("75.75")));
        assert_eq!(book.open.len(), 2, "both lots stay, each with its share left");
    }

    #[test]
    fn a_transfer_carries_the_lots_with_their_moment_and_cost_and_loses_the_networks_units() {
        let (exchange, wallet) = (Uuid::new_v4(), Uuid::new_v4());
        let buy = trade(TradeKind::Buy, exchange, "1", Some("100"), "0", 0);
        let mut sent = trade(TradeKind::Transfer, exchange, "1", None, "0.01", 5);
        sent.to_account_id = Some(wallet);
        let sale = trade(TradeKind::Sell, wallet, "0.99", Some("150"), "0", 6);
        let book = work_out(&[sale, sent, buy.clone()], &HashMap::new());

        let closed = &book.disposals[0].lots;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].trade_id, buy.id, "the lot is still the buy's");
        assert_eq!(closed[0].acquired_at, buy.occurred_at, "and was acquired when the buy was made");
        assert_eq!((closed[0].quantity, closed[0].cost), (decimal("0.99"), decimal("100")));
        assert!(book.open.is_empty(), "{:?}", book.open);
    }

    #[test]
    fn a_reward_is_a_lot_at_what_it_was_worth_and_moves_no_money() {
        let account = Uuid::new_v4();
        let reward = trade(TradeKind::Receive, account, "10", Some("0.5"), "0", 0);
        let gift = trade(TradeKind::Receive, account, "2", Some("0"), "0", 1);
        let sale = trade(TradeKind::Sell, account, "12", Some("1"), "0", 2);
        let book = work_out(&[reward, gift, sale], &HashMap::new());
        assert_eq!(book.disposals[0].lots.iter().map(|lot| lot.cost).sum::<Decimal>(), decimal("5"));
        assert_eq!(book.disposals[0].proceeds, decimal("12"));
    }

    #[test]
    fn what_arrives_at_an_instant_is_counted_before_what_leaves_then() {
        let account = Uuid::new_v4();
        let mut sale = trade(TradeKind::Sell, account, "1", Some("10"), "0", 0);
        let buy = trade(TradeKind::Buy, account, "1", Some("5"), "0", 0);
        // Whatever their ids, the buy is there for the sale to take.
        sale.id = Uuid::nil();
        let book = work_out(&[sale, buy.clone()], &HashMap::new());
        assert_eq!(book.disposals[0].lots[0].trade_id, buy.id);
        assert_eq!(book.disposals[0].lots[0].cost, decimal("5"));
    }
}
