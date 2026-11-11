//! What this service keeps and answers with.
//!
//! Every quantity, price and amount travels as a string: a JSON number is an
//! IEEE double to a browser, and a quantity of a coin is lost on the way in
//! before anything shows it (ADR 0004).

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::lots::Method;

/// What a trade is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "trade_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TradeKind {
    /// Money on the account became units of the instrument.
    Buy,
    /// Units became money on the account.
    Sell,
    /// Units left one account for another; the network took the fee.
    Transfer,
    /// Units arrived for nothing paid - a staking reward, an airdrop, a gift.
    /// No money moves; its price is what a unit was worth when it arrived.
    Receive,
}

/// A trade, as it is kept.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Trade {
    /// Stable identifier.
    pub id: Uuid,
    /// What it is.
    pub kind: TradeKind,
    /// The account it happened on - where a transfer left.
    pub account_id: Uuid,
    /// Where a transfer arrived.
    pub to_account_id: Option<Uuid>,
    /// What changed hands.
    pub instrument_id: Uuid,
    /// How many units.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity: Decimal,
    /// What one unit cost, in `currency`; a transfer has none.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "60000")]
    pub price: Option<Decimal>,
    /// The account's currency, which the price and the fee are in; a transfer
    /// has none.
    pub currency: Option<String>,
    /// What it cost to make: in `currency` for a buy or a sell, in units of
    /// the instrument for a transfer.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1")]
    pub fee: Decimal,
    /// The instant it happened.
    pub occurred_at: DateTime<Utc>,
    /// The day it happened where the person was: the day its money is in the
    /// books under.
    pub occurred_on: NaiveDate,
    /// What the person said about it.
    pub note: String,
    /// The ledger's entry for its money; a transfer moves none.
    pub entry_id: Option<Uuid>,
    /// What its money moved the account by, in `currency`: negative for a
    /// buy, positive for a sale. Worked out from the trade, as the entry is.
    #[sqlx(skip)]
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "-601")]
    pub money: Option<Decimal>,
    /// The name the client that recorded it gave it - an import's, for what
    /// it brought - so bringing it again is the same trade. Nothing for one
    /// recorded without.
    #[schema(example = "austeris-2025:transaction:17")]
    pub idempotency_key: Option<String>,
    /// When it was first recorded.
    pub created_at: DateTime<Utc>,
    /// When it was last changed.
    pub updated_at: DateTime<Utc>,
}

/// A trade being recorded: what it is, and the name the client gives it.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct NewTradeBody {
    /// What the trade is.
    #[serde(flatten)]
    pub trade: TradeBody,
    /// A name of the client's choosing, unique among the person's trades:
    /// recording under a name already used answers the trade recorded under
    /// it, as it is, rather than recording a second. What an import names
    /// what it brings by, so a file read twice is read once.
    #[schema(example = "austeris-2025:transaction:17")]
    pub idempotency_key: Option<String>,
}

/// A trade as a person states it - recording one, or saying what one is now.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct TradeBody {
    /// What it is.
    pub kind: TradeKind,
    /// The account it happened on: a broker's or a wallet's - where a
    /// transfer left.
    pub account_id: Uuid,
    /// Where a transfer arrived; a buy or a sell has none.
    pub to_account_id: Option<Uuid>,
    /// What changed hands.
    pub instrument_id: Uuid,
    /// How many units.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity: Decimal,
    /// What one unit cost, in the account's currency: a buy's and a sell's.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "60000")]
    pub price: Option<Decimal>,
    /// What it cost to make: in the account's currency for a buy or a sell,
    /// in units of the instrument for a transfer. Nothing when omitted.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "1")]
    pub fee: Option<Decimal>,
    /// The instant it happened, with the offset of where the person was:
    /// `2026-10-20T14:30:00-03:00`. The day its money is booked under is that
    /// instant's date there.
    #[schema(value_type = String, example = "2026-10-20T14:30:00-03:00")]
    pub occurred_at: DateTime<FixedOffset>,
    /// What the person says about it.
    #[serde(default)]
    pub note: String,
}

/// Units of an instrument held on an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Position {
    /// Where they are held.
    pub account_id: Uuid,
    /// What they are.
    pub instrument_id: Uuid,
    /// How many.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity: Decimal,
}

/// Whether a line of a valuation is a position or money.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "snapshot_line_kind", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    /// Money on the account, in its currency: what a trade there is paid in.
    Cash,
    /// Units of an instrument.
    Position,
}

/// One line of what an account holds, and what it is worth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Holding {
    /// A position, or money.
    pub kind: LineKind,
    /// The instrument of a position.
    pub instrument_id: Option<Uuid>,
    /// The instrument's ticker, or the money's currency.
    pub symbol: String,
    /// The instrument's name; empty for money.
    pub name: String,
    /// Units held, or the amount of money.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity: Decimal,
    /// What a unit is worth in the valuation's currency - a price, or the rate
    /// of the money's currency into it. Absent when nothing is known.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "61234.5")]
    pub price: Option<Decimal>,
    /// When that price was observed, or the day the rate was set for.
    pub priced_at: Option<DateTime<Utc>>,
    /// `quantity` at `price`; absent exactly when the price is.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "612.345")]
    pub value: Option<Decimal>,
    /// The price is older than its kind stays current for: the last known,
    /// not today's.
    pub stale: bool,
    /// What the units of a position cost, in the valuation's currency: each
    /// lot at the rate of the day it was acquired. Absent for money, and when
    /// a rate is not known.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "601")]
    pub cost: Option<Decimal>,
    /// `value` less `cost`: what the position has gained - or lost, below
    /// nothing - and not sold. Absent when either is.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "11.345")]
    pub gain: Option<Decimal>,
}

/// An account positions are held on, and what is on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AccountValue {
    /// Which account.
    pub account_id: Uuid,
    /// What the person calls it.
    pub name: String,
    /// The money first, then the positions by ticker.
    pub holdings: Vec<Holding>,
    /// What every line with a value adds up to.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1012.345")]
    pub total: Decimal,
}

/// What the portfolio held on a day, and what it was worth in one currency.
///
/// Of a day, as the books are: the money is what the accounts held at its
/// end, and the positions are the trades of that day and before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Valuation {
    /// The day.
    pub on: NaiveDate,
    /// The moment its prices are of: now, for today; the day's end, for a
    /// day past.
    pub at: DateTime<Utc>,
    /// What it is valued in.
    pub currency: String,
    /// Each account positions are held on.
    pub accounts: Vec<AccountValue>,
    /// What every line with a value adds up to.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1012.345")]
    pub total: Decimal,
    /// What is left out of the total for want of a price or a rate, by
    /// ticker or currency. A total that silently drops them looks like an
    /// answer.
    pub unvalued: Vec<String>,
    /// Something in the total is at a price older than its kind stays
    /// current for.
    pub stale: bool,
    /// What every position in the total has gained and not sold. Absent when
    /// one of them has no cost known: a sum that leaves one out looks like an
    /// answer.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "11.345")]
    pub gain: Option<Decimal>,
}

/// A valuation kept as it was, to be compared with later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Snapshot {
    /// Stable identifier.
    pub id: Uuid,
    /// What the person wrote on it.
    pub note: String,
    /// When it was taken - which may be long after the moment it shows.
    pub taken_at: DateTime<Utc>,
    /// The name the client that took it gave it, so taking it again is the
    /// same snapshot. Nothing for one taken without.
    pub idempotency_key: Option<String>,
    /// What it shows.
    #[serde(flatten)]
    pub valuation: Valuation,
}

/// A snapshot in a list: when, in what, and how much.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SnapshotSummary {
    /// Which.
    pub id: Uuid,
    /// The day it shows.
    pub on: NaiveDate,
    /// What it is valued in.
    pub currency: String,
    /// What the person wrote on it.
    pub note: String,
    /// When it was taken.
    pub taken_at: DateTime<Utc>,
    /// What it was worth.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1012.345")]
    pub total: Decimal,
    /// Something was left out of the total for want of a price.
    pub incomplete: bool,
    /// Something in the total was at a stale price.
    pub stale: bool,
}

/// How one line moved between two valuations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Difference {
    /// The account.
    pub account_id: Uuid,
    /// What the person calls it - now, or then if it is gone.
    pub account_name: String,
    /// A position, or money.
    pub kind: LineKind,
    /// The instrument of a position.
    pub instrument_id: Option<Uuid>,
    /// Its ticker, or the money's currency.
    pub symbol: String,
    /// Units, or money, in the earlier valuation; zero when there was none.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity_then: Decimal,
    /// The same in the later one.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.015")]
    pub quantity_now: Decimal,
    /// What it was worth then, when it was valued.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "600")]
    pub value_then: Option<Decimal>,
    /// What it is worth now, when it is valued.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "918.5")]
    pub value_now: Option<Decimal>,
}

/// A snapshot beside a later valuation - now, or another snapshot - and what
/// moved between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Comparison {
    /// The snapshot.
    pub snapshot: Snapshot,
    /// What it is compared with, in the snapshot's currency.
    pub against: Valuation,
    /// The snapshot `against` is, when it is one rather than now.
    pub against_id: Option<Uuid>,
    /// Every line in either, with what it was and is; the ones that did not
    /// move as well, so the two can be read side by side.
    pub differences: Vec<Difference>,
}

/// What happened to a trade, as one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "revision_action", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum RevisionAction {
    /// Recorded.
    Created,
    /// Changed.
    Changed,
    /// Deleted.
    Deleted,
}

/// Who wrote something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "revision_actor", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum WhoKind {
    /// A person, through the gateway.
    Person,
    /// A module, for a person.
    Module,
    /// Something that wrote to the database without going through austeris.
    Outside,
}

/// Who wrote something, and their name when they gave one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Who {
    /// A person, a module, or outside austeris.
    pub kind: WhoKind,
    /// The person, when it was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<Uuid>,
    /// The module's name, when it was one and gave it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
}

/// A trade as it was at a point in its history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TradeState {
    /// What it was.
    pub kind: TradeKind,
    /// The account it happened on.
    pub account_id: Uuid,
    /// Where a transfer arrived.
    pub to_account_id: Option<Uuid>,
    /// What changed hands.
    pub instrument_id: Uuid,
    /// How many units.
    #[schema(value_type = String)]
    pub quantity: String,
    /// What one unit cost.
    #[schema(value_type = Option<String>)]
    pub price: Option<String>,
    /// What the price and the fee were in.
    pub currency: Option<String>,
    /// What it cost to make.
    #[schema(value_type = String)]
    pub fee: String,
    /// The instant it happened.
    pub occurred_at: DateTime<Utc>,
    /// The day it was booked under.
    pub occurred_on: NaiveDate,
    /// What the person said about it.
    pub note: String,
}

/// One thing that happened to a trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TradeRevision {
    /// The order they happened in.
    pub id: i64,
    /// When.
    pub at: DateTime<Utc>,
    /// What.
    pub action: RevisionAction,
    /// Who.
    pub who: Who,
    /// The trade before; absent for its recording.
    pub before: Option<TradeState>,
    /// The trade after; absent for its deletion.
    pub after: Option<TradeState>,
}

/// A broker's or a wallet's account, as the portfolio counts what is on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PortfolioAccount {
    /// Which account, in the books.
    pub account_id: Uuid,
    /// What the person calls it.
    pub name: String,
    /// The money it is kept in: what its trades are paid in.
    pub currency: String,
    /// Closed: its trades are still true, and nothing new happens on it.
    pub closed: bool,
    /// How its sales and transfers out pick the lots they take.
    pub cost_method: Method,
}

/// What the portfolio is told about an account.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct AccountBody {
    /// How its sales and transfers out pick the lots they take: every gain on
    /// it is counted again this way, the ones of years past as well.
    pub cost_method: Method,
}

/// Units acquired together and still held, or what is left of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OpenLot {
    /// The buy or the receive they came from.
    pub trade_id: Uuid,
    /// When they were acquired - on whichever account, before a transfer.
    pub acquired_at: DateTime<Utc>,
    /// The day they were acquired, where the person was.
    pub acquired_on: NaiveDate,
    /// How many are left.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.005")]
    pub quantity: Decimal,
    /// What those cost, in `currency`, the buy's fee included.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "300.5")]
    pub cost: Decimal,
    /// What a unit of them cost.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "60100")]
    pub unit_cost: Decimal,
    /// The currency of the account they were acquired on.
    pub currency: String,
}

/// A position, and the lots it is made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PositionLots {
    /// Where it is held.
    pub account_id: Uuid,
    /// What it is.
    pub instrument_id: Uuid,
    /// Its ticker; the id when the market no longer knows it.
    pub symbol: String,
    /// How the account picks the lots a sale takes.
    pub method: Method,
    /// Units held: what the lots add up to.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.015")]
    pub quantity: Decimal,
    /// The lots, the oldest first.
    pub lots: Vec<OpenLot>,
}

/// Part of a lot a sale took.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ClosedLot {
    /// The buy or the receive it came from.
    pub trade_id: Uuid,
    /// When its units were acquired.
    pub acquired_at: DateTime<Utc>,
    /// The day they were acquired.
    pub acquired_on: NaiveDate,
    /// How many units of it the sale took.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.005")]
    pub quantity: Decimal,
    /// What those cost, in `currency`.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "300.5")]
    pub cost: Decimal,
    /// The currency they were acquired in.
    pub currency: String,
}

/// A sale, and what it gained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Gain {
    /// The sale.
    pub trade_id: Uuid,
    /// Where it was made.
    pub account_id: Uuid,
    /// What the person calls the account now.
    pub account_name: String,
    /// What was sold.
    pub instrument_id: Uuid,
    /// Its ticker; the id when the market no longer knows it.
    pub symbol: String,
    /// When.
    pub occurred_at: DateTime<Utc>,
    /// The day its money is booked under.
    pub occurred_on: NaiveDate,
    /// How the account picked the lots it took.
    pub method: Method,
    /// Units sold.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.01")]
    pub quantity: Decimal,
    /// What it brought in, less its fee, in the report's currency at the rate
    /// of its day. Absent when the rate is not known.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "649.35")]
    pub proceeds: Option<Decimal>,
    /// What the units it took cost, in the report's currency: each lot at the
    /// rate of the day it was acquired. Absent when a rate is not known.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "601")]
    pub cost: Option<Decimal>,
    /// `proceeds` less `cost`: below nothing for a loss. Absent when either is.
    #[serde(with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "48.35")]
    pub gain: Option<Decimal>,
    /// The lots it took, in the order it took them.
    pub lots: Vec<ClosedLot>,
}

/// What sales over a period gained, in one currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Gains {
    /// What it is counted in.
    pub currency: String,
    /// The first day of the period.
    pub from: NaiveDate,
    /// The last day of the period.
    pub to: NaiveDate,
    /// Every sale of the period, the latest first.
    pub sales: Vec<Gain>,
    /// What the sales with a gain known brought in, less their fees.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "649.35")]
    pub proceeds: Decimal,
    /// What the units they took cost.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "601")]
    pub cost: Decimal,
    /// The two, one less the other: below nothing for a loss.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "48.35")]
    pub gain: Decimal,
    /// The currencies with no rate into the report's on a day a sale or a lot
    /// needed one. The sales that needed them are listed, and left out of the
    /// sums: a sum that silently drops them looks like an answer.
    pub unvalued: Vec<String>,
}
