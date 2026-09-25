//! Turning balances in several currencies into one picture.
//!
//! A person with a guarani account, a dollar account and a crypto wallet has
//! three balances and one question: how much is that. Answering it means a rate
//! per currency, on the day being asked about - and being honest when there is
//! none, rather than converting at one and calling it the same money.

use anyhow::Result;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::model::{Balance, RateInForce};
use crate::repository;

/// How many decimal places a converted amount carries.
///
/// The same scale the columns store at. A stored amount comes back with its
/// column's scale, which is the column being honest; a *product* of two such
/// amounts has thirty-six places, which is an artefact of multiplication rather
/// than a fact about any money. Rounded once, here, so a client and a sum
/// agree - not by each reader rounding for display, which is how two views of
/// the same money come to disagree by a cent.
const SCALE: u32 = 18;

/// What a set of balances adds up to, once converted.
#[derive(Debug, Clone)]
pub struct Total {
    /// The currency everything was converted into.
    pub currency: String,
    /// The sum of every balance that could be converted.
    pub amount: Decimal,
    /// Currencies no rate was found for, so their balances are not in `amount`.
    ///
    /// Reported rather than quietly dropped: a total that silently omits a
    /// third of someone's money is worse than no total, because it looks like
    /// an answer.
    pub unconverted: Vec<String>,
    /// Currencies converted at a rate older than a long weekend, so `amount`
    /// is built on a number that may no longer be true.
    pub stale: Vec<String>,
}

/// Converts balances into one currency and adds them up.
///
/// The rate is the one in force on `as_of` - the newest not after it, so a
/// Sunday uses Friday's. A balance in a currency with no rate keeps its own
/// amount, gets no `converted`, and its currency is named in
/// [`Total::unconverted`].
///
/// # Errors
///
/// Returns an error when a rate cannot be read.
pub async fn convert(pool: &PgPool, balances: &mut [Balance], into: &str, as_of: NaiveDate) -> Result<Total> {
    let mut total = Decimal::ZERO;
    let mut unconverted: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    // One lookup per distinct currency, not per account: a person with six
    // guarani accounts asks for the guarani rate once.
    let mut currencies: Vec<String> = balances.iter().map(|balance| balance.currency.clone()).collect();
    currencies.sort_unstable();
    currencies.dedup();

    let mut rates: Vec<(String, Option<RateInForce>)> = Vec::with_capacity(currencies.len());
    for currency in currencies {
        let rate = repository::rate_in_force(pool, &currency, into, as_of).await?;
        if rate.as_ref().is_some_and(|rate| rate.stale) {
            stale.push(currency.clone());
        }
        rates.push((currency, rate));
    }

    for balance in balances.iter_mut() {
        let rate = rates
            .iter()
            .find(|(currency, _)| *currency == balance.currency)
            .and_then(|(_, rate)| rate.clone());
        if let Some(rate) = rate {
            let converted = (balance.amount * rate.rate).round_dp(SCALE);
            balance.converted = Some(converted);
            // A currency in itself needs no rate shown; one that was converted
            // says at what, and how old that was.
            balance.rate_used = (balance.currency != into).then_some(rate);
            total += converted;
        } else {
            balance.converted = None;
            balance.rate_used = None;
            if !unconverted.contains(&balance.currency) {
                unconverted.push(balance.currency.clone());
            }
        }
    }

    Ok(Total {
        currency: into.to_owned(),
        // Rounded again: the sum of rounded parts can still carry a longer
        // scale than any of them, and the total is what a person reads.
        amount: total.round_dp(SCALE),
        unconverted,
        stale,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    /// Rounding is not applied on the way into a total.
    ///
    /// A rate of 0.00013 against a guarani balance gives a long tail, and
    /// rounding each balance before adding them makes the total differ from the
    /// same money added the other way round. The presentation layer rounds; the
    /// arithmetic does not (ADR 0004).
    #[test]
    fn converting_keeps_every_digit_the_rate_produces() {
        let amount = Decimal::from_str("2500000").expect("a decimal");
        let rate = Decimal::from_str("0.00013").expect("a decimal");
        assert_eq!(amount * rate, Decimal::from_str("325.00000").expect("a decimal"));

        // A rate that does not divide evenly keeps its tail rather than
        // becoming 0.33.
        let third = Decimal::from_str("0.333333333333333333").expect("a decimal");
        assert_eq!(Decimal::from(3) * third, Decimal::from_str("0.999999999999999999").expect("a decimal"));
    }
}
