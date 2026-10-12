//! A payment or an income between one account and one category, whatever said
//! it: a typed line, a template's button, an entry recorded again.
//!
//! Three surfaces arrive at the same shape - an amount, maybe in another
//! currency than the account's, on a category, from an account - and the
//! rules for turning it into lines live here once: the signs, the conversion
//! at the stated or the day's rate, the fee. Working out what it would be is
//! kept apart from writing it, so a screen can show what a line means before
//! anyone records it and nothing is created by asking.

use austeris_common::{AppError, AppResult, Code};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::currency;
use crate::exchange::{self, Money, Plan, Summary};
use crate::model::{Account, Category, Flow, Purpose};
use crate::repository::{self, CounterpartyChoice, NewEntry, NewLine, NewPlace, Side};

/// A payment or an income, said but not yet worked out.
pub struct Payment {
    /// The day the money moved.
    pub occurred_on: NaiveDate,
    /// The account it left or arrived in.
    pub account: Account,
    /// What it was for.
    pub category: Category,
    /// Which way it went.
    pub flow: Flow,
    /// How much, always positive, in `currency`.
    pub amount: Decimal,
    /// What the amount is in: the account's own currency, or another one it
    /// was charged or paid in.
    pub currency: String,
    /// The rate it was converted at, when one was stated: how many of the
    /// account's currency one of `currency` cost. The day's rate otherwise.
    pub rate: Option<Decimal>,
    /// What a person would call it.
    pub description: String,
    /// What the category's side says.
    pub note: String,
    /// Who the money went to or came from.
    pub counterparty: Option<CounterpartyChoice>,
    /// The labels on it, by name.
    pub tags: Vec<String>,
    /// Where it happened.
    pub place: Option<NewPlace>,
    /// Held rather than posted.
    pub pending: bool,
}

/// A payment worked out: what the account moves by, and the conversion when
/// money changed currency on the way.
pub struct Priced {
    /// The payment.
    pub payment: Payment,
    /// What the account moves by, signed, in its own currency.
    pub moves: Decimal,
    /// The conversion, when the amount was not in the account's currency.
    conversion: Option<(Plan, Side, Side)>,
}

impl Payment {
    /// Works the payment out against the rates, writing nothing.
    ///
    /// The category keeps the amount as it was charged - a 10 USD subscription
    /// is 10 USD in every report kept in dollars - and the account gets what
    /// it actually moved by, in its own currency, rounded to the places that
    /// currency has. The conversion between them is where each currency
    /// balances.
    ///
    /// # Errors
    ///
    /// Refuses a rate stated for an amount already in the account's currency,
    /// an amount in a currency no rate is known for, and one worth nothing in
    /// the account's currency.
    pub async fn priced(self, pool: &PgPool) -> AppResult<Priced> {
        let account_currency = self.account.currency.clone();
        if self.currency.eq_ignore_ascii_case(&account_currency) {
            // A rate only means something for an amount in another currency;
            // stated on a plain line it would be silently ignored, and the
            // person would think it had been applied.
            if self.rate.is_some() {
                return Err(AppError::bad_request(anyhow::anyhow!(
                    "a rate converts an amount in another currency; this one is already in {account_currency}"
                ))
                .with_code(Code::LineRateWithoutCurrency, serde_json::json!({ "currency": account_currency })));
            }
            let moves = match self.flow {
                Flow::Expense => -self.amount,
                Flow::Income => self.amount,
            };
            return Ok(Priced {
                payment: self,
                moves,
                conversion: None,
            });
        }

        let foreign = self.currency.clone();
        let rate = match self.rate {
            Some(stated) => stated,
            None => {
                repository::rate_in_force(pool, &foreign, &account_currency, self.occurred_on)
                    .await?
                    .ok_or_else(|| {
                        AppError::bad_request(anyhow::anyhow!(
                            "no {foreign}->{account_currency} rate is known on or before {}; state the one you were charged, as in `@5965`",
                            self.occurred_on
                        ))
                        .with_code(
                            Code::LineNoRate,
                            serde_json::json!({ "from": foreign, "to": account_currency, "day": self.occurred_on }),
                        )
                    })?
                    .rate
            }
        };
        let in_account = currency::round(self.amount * rate, &account_currency);
        if in_account.is_zero() {
            return Err(
                AppError::bad_request(anyhow::anyhow!("{} {foreign} is worth nothing in {account_currency}", self.amount.normalize())).with_code(
                    Code::LineWorthless,
                    serde_json::json!({ "amount": self.amount.normalize().to_string(), "currency": foreign, "account_currency": account_currency }),
                ),
            );
        }

        // Spending gives the account's currency for the foreign one; earning
        // gives the foreign one for the account's.
        let in_account_money = Money {
            amount: in_account,
            currency: account_currency,
        };
        let foreign_money = Money {
            amount: self.amount,
            currency: foreign,
        };
        let (given, got, from, to, moves) = match self.flow {
            Flow::Expense => (
                in_account_money,
                foreign_money,
                Side::Account(self.account.id),
                Side::Category(self.category.id),
                -in_account,
            ),
            Flow::Income => (
                foreign_money,
                in_account_money,
                Side::Category(self.category.id),
                Side::Account(self.account.id),
                in_account,
            ),
        };
        let plan = plan(pool, given, got, self.occurred_on).await?;
        Ok(Priced {
            payment: self,
            moves,
            conversion: Some((plan, from, to)),
        })
    }
}

impl Priced {
    /// What converting it does, when money changes currency on the way.
    pub fn conversion(&self) -> Option<&Summary> {
        self.conversion.as_ref().map(|(plan, ..)| &plan.summary)
    }

    /// The entry to post, and what converting it did.
    ///
    /// The one step that may write before the entry does: a conversion that
    /// kept a fee files it under the ledger's own category, created the first
    /// time one is needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the fee category cannot be had.
    pub async fn entry(self, pool: &PgPool, owner_id: Uuid, allow_duplicate: bool) -> AppResult<(NewEntry, Option<Summary>)> {
        let payment = self.payment;
        let (lines, summary) = match self.conversion {
            None => {
                // The signs: money spent leaves the account and lands on the
                // expense category, money earned arrives in the account from
                // the income category.
                let lines = vec![
                    NewLine {
                        side: Side::Account(payment.account.id),
                        amount: self.moves,
                        currency: payment.account.currency.clone(),
                        note: String::new(),
                    },
                    NewLine {
                        side: Side::Category(payment.category.id),
                        amount: -self.moves,
                        currency: payment.account.currency.clone(),
                        note: payment.note.clone(),
                    },
                ];
                (lines, None)
            }
            Some((plan, from, to)) => {
                let mut lines = conversion_lines(pool, owner_id, &plan, from, to).await?;
                for line in &mut lines {
                    if line.side == Side::Category(payment.category.id) {
                        line.note.clone_from(&payment.note);
                    }
                }
                (lines, Some(plan.summary))
            }
        };

        let entry = NewEntry {
            occurred_on: payment.occurred_on,
            description: payment.description,
            idempotency_key: None,
            source: None,
            pending: payment.pending,
            counterparty: payment.counterparty,
            place: payment.place,
            tags: payment.tags,
            lines,
            allow_duplicate,
        };
        Ok((entry, summary))
    }
}

/// Plans a conversion of `given` into `got` against the day's rate.
pub async fn plan(pool: &PgPool, given: Money, got: Money, occurred_on: NaiveDate) -> AppResult<Plan> {
    // The reference prices what was got in what was given: how many of the
    // given currency the received amount was worth that day.
    let reference = repository::rate_in_force(pool, &got.currency, &given.currency, occurred_on).await?;
    exchange::plan(given, got, reference).map_err(AppError::bad_request)
}

/// The lines of a planned conversion, creating the fee category the first time
/// a fee needs one.
pub async fn conversion_lines(pool: &PgPool, owner_id: Uuid, plan: &Plan, from: Side, to: Side) -> AppResult<Vec<NewLine>> {
    let fee_category = if plan.has_fee() {
        Some(
            repository::purpose_category(pool, owner_id, Purpose::ExchangeFees)
                .await
                .map_err(AppError::bad_request)?
                .id,
        )
    } else {
        None
    };
    plan.lines(from, to, fee_category).map_err(AppError::internal)
}
