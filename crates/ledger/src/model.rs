//! What this service stores.

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of place money sits in.
///
/// The kinds differ in what they mean to a person, not in how they are posted
/// to: a card and a loan are both a balance that lines move. A loan simply
/// spends most of its life negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "account_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// Notes and coins.
    Cash,
    /// A current or savings account.
    Bank,
    /// A credit card: a balance that is usually owed rather than held.
    Card,
    /// A broker's account holding securities.
    Brokerage,
    /// A wallet holding crypto.
    CryptoWallet,
    /// Money placed for a term.
    Deposit,
    /// Money borrowed: a balance that is paid down rather than spent.
    Loan,
}

/// Where money sits.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Account {
    /// Stable identifier.
    pub id: Uuid,
    /// What kind of place this is.
    pub kind: AccountKind,
    /// What the person calls it.
    pub name: String,
    /// The currency it is denominated in. One per account: an account holding
    /// two has no balance anyone can state.
    pub currency: String,
    /// What was in it before austeris knew about it.
    ///
    /// A fact about the account rather than an entry: an opening balance is not
    /// a movement, and posting one would put money in reports of what was
    /// earned and spent.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1250.00")]
    pub opening_balance: Decimal,
    /// When it was closed, if it was. Closed rather than deleted: its entries
    /// are still true.
    pub closed_at: Option<DateTime<Utc>>,
}

/// Whether a category is money coming in or going out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "category_flow", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    /// Money earned.
    Income,
    /// Money spent.
    Expense,
}

/// What money is earned from or spent on.
///
/// In bookkeeping terms an income or expense account: the other side of an
/// entry whose first side is an [`Account`]. That is why a line references one
/// or the other and never both.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Category {
    /// Stable identifier.
    pub id: Uuid,
    /// The category this one sits under; `None` is a root.
    pub parent_id: Option<Uuid>,
    /// Whether this is money in or out.
    pub flow: Flow,
    /// What the person calls it.
    pub name: String,
    /// What the ledger itself files under it, when it is one the ledger made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<Purpose>,
}

/// Why the ledger keeps a category of its own.
///
/// Found by purpose rather than by name: the person may rename or move it, and
/// it stays the one the ledger files into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "category_purpose", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// What exchanging money cost beyond the day's rate: the spread a bank or
    /// an exchange office kept, and any commission on top.
    ExchangeFees,
}

impl Purpose {
    /// The name the category is created under, before anyone renames it.
    #[must_use]
    pub fn default_name(self) -> &'static str {
        match self {
            Self::ExchangeFees => "Exchange fees",
        }
    }

    /// Whether it is money in or out.
    #[must_use]
    pub fn flow(self) -> Flow {
        match self {
            Self::ExchangeFees => Flow::Expense,
        }
    }
}

/// One movement, whatever its shape.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Entry {
    /// Stable identifier.
    pub id: Uuid,
    /// The day the money moved, not the instant it was recorded.
    pub occurred_on: NaiveDate,
    /// What the person would call it: the shop, the payee, "salary".
    pub description: String,
    /// Which module produced it, or `None` when a person did.
    pub source: Option<String>,
    /// The sides of the movement. They sum to zero in every currency.
    pub lines: Vec<Line>,
}

impl Entry {
    /// What the entry is "for", as a person would state it.
    ///
    /// The positive lines of one currency: the money that arrived somewhere.
    /// Derived rather than stored, because a total kept beside the lines is a
    /// second copy of the same truth and the first edit parts them.
    #[must_use]
    pub fn total(&self, currency: &str) -> Decimal {
        self.lines
            .iter()
            .filter(|line| line.currency == currency && line.amount.is_sign_positive())
            .map(|line| line.amount)
            .sum()
    }

    /// Every currency the entry touches, in the order its lines first name them.
    #[must_use]
    pub fn currencies(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for line in &self.lines {
            if !seen.iter().any(|c| c == &line.currency) {
                seen.push(line.currency.clone());
            }
        }
        seen
    }
}

/// What a line is attributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "line_side", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum LineSide {
    /// Money moving in or out of an account, in the account's currency.
    Account,
    /// Money earned from or spent on a category.
    Category,
    /// Money changing currency. An exchange has two of these: one currency
    /// goes in, another comes out, and each still sums to zero on its own.
    Conversion,
}

/// One side of a movement.
///
/// Signed: money leaving an account is negative there and positive on the
/// category it went to, and the two cancel.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Line {
    /// Stable identifier.
    pub id: Uuid,
    /// What this line is attributed to.
    pub side: LineSide,
    /// The account this side moves, when it is an account.
    pub account_id: Option<Uuid>,
    /// The category this side is attributed to, when it is a category.
    pub category_id: Option<Uuid>,
    /// The signed amount. Serialized as a string: a JSON number reaching a
    /// browser is an IEEE double, and the value is lost before it renders
    /// (ADR 0004).
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "-45000.00")]
    pub amount: Decimal,
    /// What this line is in.
    pub currency: String,
    /// What this side was for, when the entry's own description is not enough.
    pub note: String,
}

/// What one currency was worth in another on a day.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ExchangeRate {
    /// The currency being priced.
    pub base_currency: String,
    /// The currency it is priced in.
    pub quote_currency: String,
    /// The day the rate applies to.
    pub on_date: NaiveDate,
    /// How many of `quote` one `base` buys.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.00013")]
    pub rate: Decimal,
    /// Where it came from: a source's name, or `manual`.
    pub source: String,
}

/// How old a rate may be before it is called stale, in days before the day it
/// is used for - the line's one number for it (`austeris_common::freshness`).
pub const STALE_AFTER_DAYS: i64 = austeris_common::freshness::DAILY_RATE_DAYS;

/// The rate that applies on a day, and how old it was by then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RateInForce {
    /// The currency being priced.
    pub base_currency: String,
    /// The currency it is priced in.
    pub quote_currency: String,
    /// How many of `quote` one `base` buys.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "5900.28")]
    pub rate: Decimal,
    /// The day the rate was set for - not the day it was asked for.
    pub on_date: NaiveDate,
    /// Where it came from: a source's name, `manual`, or two of those joined
    /// through a third currency when no rate for the pair itself was known.
    pub source: String,
    /// Days between `on_date` and the day it was asked for.
    pub age_days: i64,
    /// Older than [`STALE_AFTER_DAYS`]. Said outright rather than left to each
    /// reader to work out: a stale rate shown as a plain number is read as
    /// today's.
    pub stale: bool,
}

impl RateInForce {
    /// A recorded rate, as it stands on `asked_for`.
    #[must_use]
    pub fn new(rate: ExchangeRate, asked_for: NaiveDate) -> Self {
        let age_days = (asked_for - rate.on_date).num_days().max(0);
        Self {
            base_currency: rate.base_currency,
            quote_currency: rate.quote_currency,
            rate: rate.rate,
            on_date: rate.on_date,
            source: rate.source,
            age_days,
            stale: age_days > STALE_AFTER_DAYS,
        }
    }
}

/// What an account holds, as its lines add up.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Balance {
    /// Which account.
    pub account_id: Uuid,
    /// What the person calls it.
    pub name: String,
    /// What it is denominated in.
    pub currency: String,
    /// Opening balance plus every line, in the account's own currency.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1250.00")]
    pub amount: Decimal,
    /// The same, converted to the currency asked for; absent when no rate for
    /// the day was available. Absent rather than zero or equal to `amount`:
    /// either would be a number someone could add up.
    #[serde(skip_serializing_if = "Option::is_none", with = "converted")]
    #[schema(value_type = Option<String>, example = "0.16")]
    pub converted: Option<Decimal>,
    /// The rate `converted` was worked out at, with its age.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_used: Option<RateInForce>,
}

/// `Option<Decimal>` as a string, or absent.
///
/// `rust_decimal::serde::str` does not cover the `Option`, and the derive needs
/// a module that does - a number here would be a double by the time a browser
/// renders it, which is the whole point of ADR 0004.
mod converted {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};

    // `&Option<T>` rather than `Option<&T>`: this signature is what serde's
    // `with` attribute calls, not one we choose.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(value: &Option<Decimal>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(decimal) => serializer.serialize_str(&decimal.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Decimal>, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        raw.map(|text| text.parse().map_err(serde::de::Error::custom)).transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Entry, Line};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    fn line(amount: &str, currency: &str) -> Line {
        Line {
            id: Uuid::nil(),
            side: super::LineSide::Account,
            account_id: None,
            category_id: None,
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: currency.to_owned(),
            note: String::new(),
        }
    }

    fn entry(lines: Vec<Line>) -> Entry {
        Entry {
            id: Uuid::nil(),
            occurred_on: chrono::NaiveDate::from_ymd_opt(2026, 9, 17).expect("a date"),
            description: String::new(),
            source: None,
            lines,
        }
    }

    #[test]
    fn the_total_is_what_arrived_not_what_moved_in_both_directions() {
        // Summing every line gives zero, which is the one number an entry's
        // total must never be.
        let entry = entry(vec![line("-45000", "PYG"), line("45000", "PYG")]);
        assert_eq!(entry.total("PYG"), Decimal::from(45_000));
    }

    #[test]
    fn a_rate_is_stale_only_once_it_is_older_than_a_long_weekend() {
        let rate = |on: u32| super::ExchangeRate {
            base_currency: "USD".to_owned(),
            quote_currency: "PYG".to_owned(),
            on_date: chrono::NaiveDate::from_ymd_opt(2026, 9, on).expect("a date"),
            rate: Decimal::from(5_900),
            source: "bcp".to_owned(),
        };
        let monday = chrono::NaiveDate::from_ymd_opt(2026, 9, 28).expect("a date");

        // Friday's rate on Monday is the ordinary case, and so is Thursday's
        // after a Friday holiday.
        let friday = super::RateInForce::new(rate(25), monday);
        assert_eq!((friday.age_days, friday.stale), (3, false));
        let thursday = super::RateInForce::new(rate(24), monday);
        assert_eq!((thursday.age_days, thursday.stale), (4, false));

        // A day more, and nothing has arrived for longer than any calendar
        // explains.
        let wednesday = super::RateInForce::new(rate(23), monday);
        assert_eq!((wednesday.age_days, wednesday.stale), (5, true));
    }

    #[test]
    fn a_split_receipt_totals_the_whole_receipt() {
        // One payment, three categories: the total is the payment, not any one
        // of the parts.
        let entry = entry(vec![line("-50000", "PYG"), line("20000", "PYG"), line("20000", "PYG"), line("10000", "PYG")]);
        assert_eq!(entry.total("PYG"), Decimal::from(50_000));
    }

    #[test]
    fn each_currency_of_an_exchange_totals_on_its_own() {
        // Buying 10 USD for 75000 PYG: the wallet gives PYG to the conversion,
        // the conversion gives USD to the dollar account. Neither total is the
        // other converted - the entry states both, which is what makes the
        // rate a fact of the entry rather than a conversion applied while
        // reading.
        let entry = entry(vec![line("-75000", "PYG"), line("75000", "PYG"), line("-10", "USD"), line("10", "USD")]);
        assert_eq!(entry.total("PYG"), Decimal::from(75_000));
        assert_eq!(entry.total("USD"), Decimal::from(10));
        assert_eq!(entry.currencies(), vec!["PYG".to_owned(), "USD".to_owned()]);
    }
}
