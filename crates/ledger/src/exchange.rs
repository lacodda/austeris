//! Money that changes currency between leaving one side and arriving at another.
//!
//! Three things a person does are this one shape: changing guaranies for
//! dollars at an exchange office, paying a 10 USD subscription from a guarani
//! card, and being paid in dollars into a guarani account. In each, an amount
//! leaves one side in one currency and a different amount arrives at another
//! side in another. The entry keeps both amounts as they happened, and the
//! conversion lines between them are what lets each currency still sum to zero.
//!
//! What the deal cost beyond the day's rate is a third line. The day's rate is
//! the reference - a central bank's, or one a person recorded - and the
//! difference between what was given and what the received amount was worth at
//! it is what the bank or the exchange office kept. It is filed under a category
//! of its own so "how much did exchanging cost me this year" is a sum, the same
//! way broker commissions are.
//!
//! Pure arithmetic: nothing here reads the database. The caller finds the
//! reference rate and the category; this decides the lines.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::currency;
use crate::model::RateInForce;
use crate::repository::{NewLine, Side};

/// An amount in a currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Money {
    /// How much.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "760000")]
    pub amount: Decimal,
    /// In what.
    pub currency: String,
}

/// The rate a deal was actually done at, as the two amounts imply it.
///
/// Derived, never stored: the entry holds the two amounts, and a rate kept
/// beside them is a second copy that the first correction parts from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DealRate {
    /// The currency one unit of which is priced - whichever of the two is worth
    /// more, so the rate reads the way an exchange office's board does.
    pub base: String,
    /// The currency it is priced in.
    pub quote: String,
    /// How many of `quote` one `base` cost in this deal.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "5965")]
    pub rate: Decimal,
}

/// Why a conversion carries no fee line even though it had a counterparty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoFee {
    /// No rate for the pair was known on or before the day.
    NoReference,
    /// The rate known was too old to measure a spread against: a week-old rate
    /// would call the currency's own movement a fee.
    StaleReference,
}

/// What a conversion did, for the person who asked for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Summary {
    /// What left the first side.
    pub given: Money,
    /// What arrived at the second.
    pub got: Money,
    /// The rate the two amounts imply.
    pub deal: DealRate,
    /// The day's rate the deal was measured against, when there was one -
    /// priced the same way round as `deal`, so the two read side by side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<RateInForce>,
    /// What was given beyond what the received amount was worth at the
    /// reference, in the given currency. Negative when the deal beat the
    /// reference. Absent when it could not be measured, or came to nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<Money>,
    /// Why there is no fee, when the reason is not that the deal matched the
    /// reference exactly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_fee: Option<NoFee>,
}

/// A conversion worked out, before anything is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// What the person will be told.
    pub summary: Summary,
    /// What the received amount was worth in the given currency at the
    /// reference - the given amount itself when there was none.
    fair: Decimal,
}

impl Plan {
    /// Whether writing this needs a category for the fee.
    #[must_use]
    pub fn has_fee(&self) -> bool {
        self.summary.fee.is_some()
    }

    /// The lines of the entry: the first side gives, the conversion turns one
    /// currency into the other, the second side receives, and the fee - when
    /// there is one - is what the first side gave beyond the fair amount.
    ///
    /// # Errors
    ///
    /// Returns an error when the plan has a fee and no category was given for
    /// it: dropping the line would leave the entry unbalanced, and folding it
    /// into the conversion would hide it.
    pub fn lines(&self, from: Side, to: Side, fee_category: Option<Uuid>) -> anyhow::Result<Vec<NewLine>> {
        let given = &self.summary.given;
        let got = &self.summary.got;

        let mut lines = vec![NewLine {
            side: from,
            amount: -given.amount,
            currency: given.currency.clone(),
            note: String::new(),
        }];
        if let Some(fee) = &self.summary.fee {
            let category = fee_category.ok_or_else(|| anyhow::anyhow!("a conversion with a fee needs a category to file it under"))?;
            lines.push(NewLine {
                side: Side::Category(category),
                amount: fee.amount,
                currency: fee.currency.clone(),
                note: "exchange fee".to_owned(),
            });
        }
        lines.extend([
            NewLine {
                side: Side::Conversion,
                amount: self.fair,
                currency: given.currency.clone(),
                note: String::new(),
            },
            NewLine {
                side: Side::Conversion,
                amount: -got.amount,
                currency: got.currency.clone(),
                note: String::new(),
            },
            NewLine {
                side: to,
                amount: got.amount,
                currency: got.currency.clone(),
                note: String::new(),
            },
        ]);
        Ok(lines)
    }
}

/// Why a conversion cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// Both amounts have to be something.
    #[error("an exchange gives something and gets something; both amounts are positive")]
    NotPositive,
    /// Both sides in one currency is a transfer, not an exchange.
    #[error("both sides are in {0}; money that stays in one currency is a transfer")]
    SameCurrency(String),
    /// The reference makes the received amount worth nothing in the given
    /// currency - it is too small to convert at all.
    #[error("{0} is too small to be worth anything in {1}")]
    TooSmall(String, String),
    /// The reference rate is not for this pair.
    #[error("a {0}->{1} rate cannot measure a conversion from {2} into {3}")]
    WrongReference(String, String, String, String),
}

/// Works out a conversion of `given` into `got`.
///
/// `reference` is the rate from `got`'s currency into `given`'s on the day -
/// how many of what was given one unit of what was got is worth. A fee is
/// measured against it only when it is fresh; the fair amount is rounded to the
/// given currency's places, and the fee takes whatever rounding leaves, so the
/// lines always balance to the unit.
///
/// # Errors
///
/// Returns [`Refused`] when an amount is not positive, both sides are in one
/// currency, the reference is for another pair, or the received amount is too
/// small to be worth a unit of the given currency.
pub fn plan(given: Money, got: Money, reference: Option<RateInForce>) -> Result<Plan, Refused> {
    if !given.amount.is_sign_positive() || given.amount.is_zero() || !got.amount.is_sign_positive() || got.amount.is_zero() {
        return Err(Refused::NotPositive);
    }
    if given.currency == got.currency {
        return Err(Refused::SameCurrency(given.currency));
    }
    if let Some(rate) = &reference
        && (rate.base_currency != got.currency || rate.quote_currency != given.currency)
    {
        return Err(Refused::WrongReference(
            rate.base_currency.clone(),
            rate.quote_currency.clone(),
            given.currency.clone(),
            got.currency.clone(),
        ));
    }

    let (fair, fee, no_fee) = match &reference {
        None => (given.amount, None, Some(NoFee::NoReference)),
        Some(rate) if rate.stale => (given.amount, None, Some(NoFee::StaleReference)),
        Some(rate) => {
            let fair = currency::round(got.amount * rate.rate, &given.currency);
            if fair.is_zero() {
                return Err(Refused::TooSmall(
                    format!("{} {}", got.amount.normalize(), got.currency),
                    given.currency.clone(),
                ));
            }
            let fee = given.amount - fair;
            let fee = (!fee.is_zero()).then(|| Money {
                amount: fee,
                currency: given.currency.clone(),
            });
            (fair, fee, None)
        }
    };

    let deal = deal_rate(&given, &got);
    let reference = reference.map(|rate| {
        if rate.base_currency == deal.base {
            shown(rate)
        } else {
            shown(inverted(rate))
        }
    });
    Ok(Plan {
        summary: Summary {
            given,
            got,
            deal,
            reference,
            fee,
            no_fee,
        },
        fair,
    })
}

/// A rate read the other way round, for showing beside a deal priced that way.
fn inverted(rate: RateInForce) -> RateInForce {
    RateInForce {
        base_currency: rate.quote_currency,
        quote_currency: rate.base_currency,
        rate: Decimal::ONE / rate.rate,
        ..rate
    }
}

/// A rate as a person reads it: ten places, which is more than any rate is
/// quoted to. The fee was measured with every digit; what is shown is the
/// 5900.28 the bank published, not the 5900.2799999... that reading an
/// inverted rate back gives, nor the twenty-seven places of a cross rate.
fn shown(rate: RateInForce) -> RateInForce {
    RateInForce {
        rate: rate.rate.round_dp(10).normalize(),
        ..rate
    }
}

/// The rate two amounts imply, priced in the currency worth less.
fn deal_rate(given: &Money, got: &Money) -> DealRate {
    // Ten places is more than any exchange office quotes to; the division
    // itself would otherwise carry twenty-odd digits of arithmetic noise.
    let (base, quote, rate) = if given.amount >= got.amount {
        (&got.currency, &given.currency, given.amount / got.amount)
    } else {
        (&given.currency, &got.currency, got.amount / given.amount)
    };
    DealRate {
        base: base.clone(),
        quote: quote.clone(),
        rate: rate.round_dp(10).normalize(),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{Money, NoFee, Refused, plan};
    use crate::model::{ExchangeRate, RateInForce};
    use crate::repository::Side;

    fn money(amount: &str, currency: &str) -> Money {
        Money {
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: currency.to_owned(),
        }
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, d).expect("a date")
    }

    /// The day's rate from `base` into `quote`, set on `set` and asked for on
    /// the 25th.
    fn reference(base: &str, quote: &str, rate: &str, set: u32) -> RateInForce {
        RateInForce::new(
            ExchangeRate {
                base_currency: base.to_owned(),
                quote_currency: quote.to_owned(),
                on_date: day(set),
                rate: Decimal::from_str(rate).expect("a decimal"),
                source: "bcp".to_owned(),
            },
            day(25),
        )
    }

    /// Sums each currency's lines, which is what the database checks.
    fn sums(lines: &[crate::repository::NewLine]) -> Vec<(String, Decimal)> {
        let mut out: Vec<(String, Decimal)> = Vec::new();
        for line in lines {
            match out.iter_mut().find(|(currency, _)| *currency == line.currency) {
                Some((_, sum)) => *sum += line.amount,
                None => out.push((line.currency.clone(), line.amount)),
            }
        }
        out
    }

    #[test]
    fn what_the_exchange_office_kept_is_a_line_of_its_own() {
        // 60000 PYG for 10 USD on a day the central bank said 5900.28.
        let plan = plan(money("60000", "PYG"), money("10", "USD"), Some(reference("USD", "PYG", "5900.28", 24))).expect("a plan");
        let fee = plan.summary.fee.clone().expect("a fee");
        // Ten dollars were worth 59003 guaranies; the other 997 were kept.
        assert_eq!(fee, money("997", "PYG"));
        assert_eq!(plan.summary.deal.base, "USD");
        assert_eq!(plan.summary.deal.rate, Decimal::from(6_000));

        let lines = plan
            .lines(Side::Account(Uuid::nil()), Side::Account(Uuid::nil()), Some(Uuid::nil()))
            .expect("lines");
        assert_eq!(lines.len(), 5);
        assert!(sums(&lines).iter().all(|(_, sum)| sum.is_zero()), "{:?}", sums(&lines));
        let conversion: Vec<_> = lines.iter().filter(|line| line.side == Side::Conversion).collect();
        assert_eq!(conversion[0].amount, Decimal::from(59_003));
        assert_eq!(conversion[1].amount, Decimal::from(-10));
    }

    #[test]
    fn selling_the_stronger_currency_measures_the_fee_in_it() {
        // 1000 USD of salary arrive as 5850000 PYG where 5900280 were fair: the
        // fee is in dollars, the currency that was given, rounded to cents.
        let plan = plan(money("1000", "USD"), money("5850000", "PYG"), Some(reference("PYG", "USD", "0.000169483", 24))).expect("a plan");
        let fee = plan.summary.fee.clone().expect("a fee");
        assert_eq!(fee.currency, "USD");
        assert_eq!(fee.amount, Decimal::from_str("8.52").expect("a decimal"));
        assert_eq!(plan.summary.deal.base, "USD");
        assert_eq!(plan.summary.deal.rate, Decimal::from(5_850));
        // The fee needed the reference as dollars per guarani; it is shown the
        // way the deal is, guaranies per dollar.
        let shown = plan.summary.reference.clone().expect("a reference");
        assert_eq!((shown.base_currency.as_str(), shown.quote_currency.as_str()), ("USD", "PYG"));
        assert_eq!(shown.rate, Decimal::from_str("5900.2967849283").expect("a decimal"));

        let lines = plan
            .lines(Side::Category(Uuid::nil()), Side::Account(Uuid::nil()), Some(Uuid::nil()))
            .expect("lines");
        assert!(sums(&lines).iter().all(|(_, sum)| sum.is_zero()), "{:?}", sums(&lines));
    }

    #[test]
    fn a_deal_better_than_the_reference_is_a_negative_fee_not_a_hidden_one() {
        let plan = plan(money("58000", "PYG"), money("10", "USD"), Some(reference("USD", "PYG", "5900.28", 24))).expect("a plan");
        assert_eq!(plan.summary.fee, Some(money("-1003", "PYG")));
    }

    #[test]
    fn a_deal_at_the_reference_has_no_fee_line() {
        let plan = plan(money("59003", "PYG"), money("10", "USD"), Some(reference("USD", "PYG", "5900.28", 24))).expect("a plan");
        assert_eq!(plan.summary.fee, None);
        assert_eq!(plan.summary.no_fee, None);
        let lines = plan.lines(Side::Account(Uuid::nil()), Side::Account(Uuid::nil()), None).expect("lines");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn a_stale_or_missing_reference_measures_no_fee_and_says_why() {
        let stale = plan(money("60000", "PYG"), money("10", "USD"), Some(reference("USD", "PYG", "5900.28", 10))).expect("a plan");
        assert_eq!((stale.summary.fee.clone(), stale.summary.no_fee), (None, Some(NoFee::StaleReference)));

        let missing = plan(money("60000", "PYG"), money("10", "USD"), None).expect("a plan");
        assert_eq!((missing.summary.fee.clone(), missing.summary.no_fee), (None, Some(NoFee::NoReference)));
        // The whole given amount goes through the conversion, so the entry
        // still balances without a fee line.
        let lines = missing.lines(Side::Account(Uuid::nil()), Side::Account(Uuid::nil()), None).expect("lines");
        assert!(sums(&lines).iter().all(|(_, sum)| sum.is_zero()));
    }

    #[test]
    fn a_fee_without_a_category_is_refused_rather_than_dropped() {
        let plan = plan(money("60000", "PYG"), money("10", "USD"), Some(reference("USD", "PYG", "5900.28", 24))).expect("a plan");
        assert!(plan.lines(Side::Account(Uuid::nil()), Side::Account(Uuid::nil()), None).is_err());
    }

    #[test]
    fn what_is_not_an_exchange_is_refused() {
        assert_eq!(
            plan(money("100", "PYG"), money("100", "PYG"), None),
            Err(Refused::SameCurrency("PYG".to_owned()))
        );
        assert_eq!(plan(money("0", "PYG"), money("10", "USD"), None), Err(Refused::NotPositive));
        assert_eq!(plan(money("100", "PYG"), money("-10", "USD"), None), Err(Refused::NotPositive));
        assert!(matches!(
            plan(money("100", "PYG"), money("10", "USD"), Some(reference("PYG", "USD", "0.00017", 24))),
            Err(Refused::WrongReference(..))
        ));
        // A hundredth of a yen is worth less than half a guarani.
        assert!(matches!(
            plan(money("1", "PYG"), money("0.01", "JPY"), Some(reference("JPY", "PYG", "37.14", 24))),
            Err(Refused::TooSmall(..))
        ));
    }
}
