//! An entry read as one payment: from one account, on one category.
//!
//! What a template is made of and what is converted again when a payment in
//! another currency is recorded a second time. Pure: the entry's lines say it,
//! and nothing here reads the database.

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::model::{Entry, Flow, LineSide};

/// One payment, as an entry's lines say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paid {
    /// The account it left or arrived in.
    pub account_id: Uuid,
    /// What it was for.
    pub category_id: Uuid,
    /// Which way the money went.
    pub flow: Flow,
    /// What the category got, always positive, in `currency`.
    pub amount: Decimal,
    /// What it was charged or paid in.
    pub currency: String,
    /// What the category's side said.
    pub note: String,
}

/// Why an entry is not one payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotOne {
    /// Two accounts: a transfer or an exchange.
    #[error("this entry moves money between accounts")]
    BetweenAccounts,
    /// No account at all.
    #[error("this entry moves no account")]
    NoAccount,
    /// Several categories: a split receipt.
    #[error("this entry is split across categories")]
    Split,
    /// No category beside the fee.
    #[error("this entry is on no category")]
    NoCategory,
}

/// The payment an entry is, when it is one: one account line, one category
/// line beside the ledger's own fee, and the conversion between them when the
/// two are in different currencies.
///
/// The category's side is what was paid - in the currency it was charged in -
/// and its sign says which way: positive on the category is money spent.
///
/// # Errors
///
/// Returns [`NotOne`] for a transfer, an exchange, a split receipt, and an
/// entry with no category.
pub fn of(entry: &Entry, fee_category: Option<Uuid>) -> Result<Paid, NotOne> {
    let accounts: Vec<_> = entry.lines.iter().filter(|line| line.side == LineSide::Account).collect();
    let account = match accounts.as_slice() {
        [only] => only,
        [] => return Err(NotOne::NoAccount),
        _ => return Err(NotOne::BetweenAccounts),
    };
    let categories: Vec<_> = entry
        .lines
        .iter()
        .filter(|line| line.side == LineSide::Category && line.category_id != fee_category)
        .collect();
    let category = match categories.as_slice() {
        [only] => only,
        [] => return Err(NotOne::NoCategory),
        _ => return Err(NotOne::Split),
    };
    // The schema holds a line to naming what its side says; a row that did
    // not would be no account and no category.
    let (Some(account_id), Some(category_id)) = (account.account_id, category.category_id) else {
        return Err(NotOne::NoAccount);
    };
    Ok(Paid {
        account_id,
        category_id,
        flow: if category.amount.is_sign_positive() { Flow::Expense } else { Flow::Income },
        amount: category.amount.abs().normalize(),
        currency: category.currency.clone(),
        note: category.note.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{NotOne, Paid, of};
    use crate::model::{Entry, EntryStatus, Flow, Line, LineSide};

    fn line(side: LineSide, id: Uuid, amount: &str, currency: &str) -> Line {
        Line {
            id: Uuid::new_v4(),
            side,
            account_id: (side == LineSide::Account).then_some(id),
            category_id: (side == LineSide::Category).then_some(id),
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: currency.to_owned(),
            note: String::new(),
        }
    }

    fn entry(lines: Vec<Line>) -> Entry {
        Entry {
            id: Uuid::new_v4(),
            occurred_on: NaiveDate::from_ymd_opt(2026, 10, 11).expect("a date"),
            status: EntryStatus::Cleared,
            cleared_on: None,
            description: String::new(),
            source: None,
            counterparty: None,
            place: None,
            tags: Vec::new(),
            lines,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn a_payment_in_another_currency_is_what_the_category_was_charged_and_not_the_fee() {
        let (card, streaming, fees) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let paid = of(
            &entry(vec![
                line(LineSide::Account, card, "-60300", "PYG"),
                line(LineSide::Category, fees, "650", "PYG"),
                line(LineSide::Conversion, Uuid::nil(), "59650", "PYG"),
                line(LineSide::Conversion, Uuid::nil(), "-10", "USD"),
                line(LineSide::Category, streaming, "10", "USD"),
            ]),
            Some(fees),
        )
        .expect("one payment");
        assert_eq!(
            paid,
            Paid {
                account_id: card,
                category_id: streaming,
                flow: Flow::Expense,
                amount: Decimal::from(10),
                currency: "USD".to_owned(),
                note: String::new(),
            }
        );
    }

    #[test]
    fn money_earned_is_an_income_and_its_amount_is_still_positive() {
        let (bank, salary) = (Uuid::new_v4(), Uuid::new_v4());
        let paid = of(
            &entry(vec![
                line(LineSide::Account, bank, "2500000", "PYG"),
                line(LineSide::Category, salary, "-2500000", "PYG"),
            ]),
            None,
        )
        .expect("one payment");
        assert_eq!((paid.flow, paid.amount), (Flow::Income, Decimal::from(2_500_000)));
    }

    #[test]
    fn a_split_a_transfer_and_an_exchange_are_not_one_payment() {
        let (cash, card, food, fun) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let split = entry(vec![
            line(LineSide::Account, cash, "-30000", "PYG"),
            line(LineSide::Category, food, "20000", "PYG"),
            line(LineSide::Category, fun, "10000", "PYG"),
        ]);
        assert_eq!(of(&split, None), Err(NotOne::Split));

        let transfer = entry(vec![
            line(LineSide::Account, cash, "-30000", "PYG"),
            line(LineSide::Account, card, "30000", "PYG"),
        ]);
        assert_eq!(of(&transfer, None), Err(NotOne::BetweenAccounts));

        let exchange = entry(vec![
            line(LineSide::Account, cash, "-600000", "PYG"),
            line(LineSide::Conversion, Uuid::nil(), "600000", "PYG"),
            line(LineSide::Conversion, Uuid::nil(), "-100", "USD"),
            line(LineSide::Account, card, "100", "USD"),
        ]);
        assert_eq!(of(&exchange, None), Err(NotOne::BetweenAccounts));
    }
}
