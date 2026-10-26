//! Snapshots: the portfolio as it was at a moment, kept, and set beside a
//! later one.
//!
//! The 2025 tracker kept a list of what was held and answered, for each, how
//! the holdings had moved since. Kept here as rows of a valuation - what each
//! account held and what it was worth - so a snapshot taken last spring still
//! reads as it read then after a trade of that time is corrected, and the
//! comparison says both how much more is held and how much more it is worth.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::model::{Difference, LineKind, Valuation};

/// What makes two lines the same line: the account, and the instrument or
/// the money's currency.
type Key = (Uuid, LineKind, Option<Uuid>, String);

/// Every line in either valuation, with what it was in the earlier and what it
/// is in the later - in the order an account is read, and with the account's
/// name as the later one has it.
#[must_use]
pub fn differences(then: &Valuation, now: &Valuation) -> Vec<Difference> {
    let mut lines: BTreeMap<Key, Difference> = BTreeMap::new();
    let mut place = |valuation: &Valuation, earlier: bool| {
        for account in &valuation.accounts {
            for holding in &account.holdings {
                // A position is its instrument whatever it is called; money is
                // its currency.
                let symbol = if holding.kind == LineKind::Cash {
                    holding.symbol.clone()
                } else {
                    String::new()
                };
                let line = lines
                    .entry((account.account_id, holding.kind, holding.instrument_id, symbol))
                    .or_insert_with(|| Difference {
                        account_id: account.account_id,
                        account_name: account.name.clone(),
                        kind: holding.kind,
                        instrument_id: holding.instrument_id,
                        symbol: holding.symbol.clone(),
                        quantity_then: Decimal::ZERO,
                        quantity_now: Decimal::ZERO,
                        value_then: None,
                        value_now: None,
                    });
                if earlier {
                    line.quantity_then = holding.quantity;
                    line.value_then = holding.value;
                } else {
                    line.account_name.clone_from(&account.name);
                    line.symbol.clone_from(&holding.symbol);
                    line.quantity_now = holding.quantity;
                    line.value_now = holding.value;
                }
            }
        }
    };
    place(then, true);
    place(now, false);

    // An account renamed since is called what it is called now, on the lines
    // it no longer has as well.
    let mut out: Vec<Difference> = lines.into_values().collect();
    for line in &mut out {
        if let Some(account) = now.accounts.iter().find(|account| account.account_id == line.account_id) {
            line.account_name.clone_from(&account.name);
        }
    }
    out.sort_by(|a, b| {
        a.account_name
            .to_lowercase()
            .cmp(&b.account_name.to_lowercase())
            .then_with(|| a.account_id.cmp(&b.account_id))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    out
}

/// The day a valuation is asked of: a day past, or now when none is given.
///
/// # Errors
///
/// Returns the reason when the day has not begun anywhere yet.
pub fn day(on: Option<NaiveDate>) -> Result<Option<NaiveDate>, &'static str> {
    match on {
        // A day ahead of UTC's is today somewhere east of it.
        Some(day) if day > Utc::now().date_naive() + Days::new(1) => Err("a day that has not happened has no value yet"),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::Utc;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::differences;
    use crate::model::{AccountValue, Holding, LineKind, Valuation};
    use crate::value::assemble;

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str(text).expect("a decimal")
    }

    fn position(instrument: Uuid, symbol: &str, quantity: &str, value: &str) -> Holding {
        Holding {
            kind: LineKind::Position,
            instrument_id: Some(instrument),
            symbol: symbol.to_owned(),
            name: String::new(),
            quantity: decimal(quantity),
            price: Some(decimal("1")),
            priced_at: None,
            value: Some(decimal(value)),
            stale: false,
        }
    }

    fn valuation(account: Uuid, name: &str, holdings: Vec<Holding>) -> Valuation {
        assemble(
            Utc::now().date_naive(),
            Utc::now(),
            "USD".to_owned(),
            vec![AccountValue {
                account_id: account,
                name: name.to_owned(),
                holdings,
                total: Decimal::ZERO,
            }],
        )
    }

    #[test]
    fn what_was_held_and_what_is_are_set_side_by_side() {
        let account = Uuid::new_v4();
        let (btc, eth, sol) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let then = valuation(
            account,
            "Exchange",
            vec![position(btc, "BTC", "0.01", "600"), position(eth, "ETH", "1", "2500")],
        );
        let now = valuation(account, "Binance", vec![position(btc, "BTC", "0.015", "975"), position(sol, "SOL", "3", "450")]);

        let moved = differences(&then, &now);
        let by_symbol = |symbol: &str| moved.iter().find(|line| line.symbol == symbol).expect("a line");
        assert_eq!(moved.len(), 3, "a line in either is a line");
        assert_eq!(by_symbol("BTC").quantity_then, decimal("0.01"));
        assert_eq!(by_symbol("BTC").quantity_now, decimal("0.015"));
        // Sold since: held then, nothing now.
        assert_eq!(by_symbol("ETH").quantity_now, Decimal::ZERO);
        assert_eq!(by_symbol("ETH").value_now, None);
        // Bought since.
        assert_eq!(by_symbol("SOL").quantity_then, Decimal::ZERO);
        assert_eq!(by_symbol("SOL").value_then, None);
        assert!(moved.iter().all(|line| line.account_name == "Binance"), "the account is named as it is now");
    }
}
