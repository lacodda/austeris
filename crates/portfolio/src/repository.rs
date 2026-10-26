//! Reading and writing this service's schema.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::model::{
    AccountValue, Holding, LineKind, Position, RevisionAction, Snapshot, SnapshotSummary, Trade, TradeKind, TradeRevision, TradeState, Valuation, Who, WhoKind,
};

/// A trade about to be written, every field checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTrade {
    /// What it is.
    pub kind: TradeKind,
    /// The account it happened on.
    pub account_id: Uuid,
    /// Where a transfer arrived.
    pub to_account_id: Option<Uuid>,
    /// What changed hands.
    pub instrument_id: Uuid,
    /// How many units.
    pub quantity: Decimal,
    /// What one unit cost.
    pub price: Option<Decimal>,
    /// What the price and the fee are in.
    pub currency: Option<String>,
    /// What it cost to make.
    pub fee: Decimal,
    /// The instant it happened.
    pub occurred_at: DateTime<Utc>,
    /// The day its money is booked under.
    pub occurred_on: NaiveDate,
    /// What the person said about it.
    pub note: String,
}

/// The columns a trade is read with.
const TRADE_COLUMNS: &str =
    "id, kind, account_id, to_account_id, instrument_id, quantity, price, currency, fee, occurred_at, occurred_on, note, entry_id, created_at, updated_at";

/// Writes a new trade.
///
/// # Errors
///
/// Returns an error when a rule of the schema refuses it - a position below
/// nothing - or the write fails.
pub async fn insert_trade(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, new: &NewTrade) -> Result<Trade> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO trades (owner_id, kind, account_id, to_account_id, instrument_id, quantity, price, currency, fee, occurred_at, occurred_on, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING {TRADE_COLUMNS}"
    )))
    .bind(owner_id)
    .bind(new.kind)
    .bind(new.account_id)
    .bind(new.to_account_id)
    .bind(new.instrument_id)
    .bind(new.quantity)
    .bind(new.price)
    .bind(new.currency.as_deref())
    .bind(new.fee)
    .bind(new.occurred_at)
    .bind(new.occurred_on)
    .bind(&new.note)
    .fetch_one(&mut **transaction)
    .await
    .context("recording a trade")
}

/// Says what a trade is now.
///
/// # Errors
///
/// Returns an error when a rule of the schema refuses it, or the write fails.
pub async fn replace_trade(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid, new: &NewTrade) -> Result<Trade> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE trades
            SET kind = $3, account_id = $4, to_account_id = $5, instrument_id = $6, quantity = $7, price = $8, currency = $9,
                fee = $10, occurred_at = $11, occurred_on = $12, note = $13, updated_at = now(),
                -- A transfer moves no money; whatever entry the trade had is
                -- taken back in the same transaction.
                entry_id = CASE WHEN $3 = 'transfer' THEN NULL ELSE entry_id END
          WHERE owner_id = $1 AND id = $2
         RETURNING {TRADE_COLUMNS}"
    )))
    .bind(owner_id)
    .bind(id)
    .bind(new.kind)
    .bind(new.account_id)
    .bind(new.to_account_id)
    .bind(new.instrument_id)
    .bind(new.quantity)
    .bind(new.price)
    .bind(new.currency.as_deref())
    .bind(new.fee)
    .bind(new.occurred_at)
    .bind(new.occurred_on)
    .bind(&new.note)
    .fetch_one(&mut **transaction)
    .await
    .context("writing what a trade is now")
}

/// Writes down the ledger's entry for a trade's money, or that it has none.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn set_entry(transaction: &mut Transaction<'_, Postgres>, id: Uuid, entry_id: Option<Uuid>) -> Result<()> {
    sqlx::query("UPDATE trades SET entry_id = $2 WHERE id = $1")
        .bind(id)
        .bind(entry_id)
        .execute(&mut **transaction)
        .await
        .context("noting the entry of a trade's money")?;
    Ok(())
}

/// A trade of this person's, locked for the rest of the transaction.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn trade_for_update(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid) -> Result<Option<Trade>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TRADE_COLUMNS} FROM trades WHERE owner_id = $1 AND id = $2 FOR UPDATE"
    )))
    .bind(owner_id)
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
    .context("finding a trade")
}

/// Deletes a trade.
///
/// # Errors
///
/// Returns an error when a rule of the schema refuses it - a later sale left
/// with nothing to sell - or the write fails.
pub async fn delete_trade(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM trades WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(&mut **transaction)
        .await
        .context("deleting a trade")?;
    Ok(deleted.rows_affected() > 0)
}

/// One trade of this person's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn trade(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Trade>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TRADE_COLUMNS} FROM trades WHERE owner_id = $1 AND id = $2"
    )))
    .bind(owner_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading a trade")
}

/// Which trades to read.
#[derive(Debug, Clone, Default)]
pub struct TradeFilter {
    /// Only those on this account, or arriving at it.
    pub account_id: Option<Uuid>,
    /// Only those of this instrument.
    pub instrument_id: Option<Uuid>,
    /// How many.
    pub limit: i64,
    /// How many to skip.
    pub offset: i64,
}

/// A person's trades, the latest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn trades(pool: &PgPool, owner_id: Uuid, filter: &TradeFilter) -> Result<Vec<Trade>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TRADE_COLUMNS} FROM trades
          WHERE owner_id = $1
            AND ($2::uuid IS NULL OR account_id = $2 OR to_account_id = $2)
            AND ($3::uuid IS NULL OR instrument_id = $3)
          ORDER BY occurred_at DESC, created_at DESC, id DESC
          LIMIT $4 OFFSET $5"
    )))
    .bind(owner_id)
    .bind(filter.account_id)
    .bind(filter.instrument_id)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
    .context("listing trades")
}

/// What a person's trades add up to: every position not empty, by account and
/// instrument - all of them, or those of a day and before.
///
/// Worked out, never stored: a quantity kept beside the trades is a second
/// copy of the same truth, and the first edit parts them.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn positions(pool: &PgPool, owner_id: Uuid, on: Option<NaiveDate>) -> Result<Vec<Position>> {
    sqlx::query_as(
        "SELECT account_id, instrument_id, sum(delta) AS quantity
           FROM (SELECT account_id, instrument_id, CASE WHEN kind = 'buy' THEN quantity ELSE -quantity END AS delta
                   FROM trades WHERE owner_id = $1 AND ($2::date IS NULL OR occurred_on <= $2)
                 UNION ALL
                 SELECT to_account_id, instrument_id, quantity - fee
                   FROM trades WHERE owner_id = $1 AND kind = 'transfer' AND ($2::date IS NULL OR occurred_on <= $2)) AS moves
          GROUP BY account_id, instrument_id
         HAVING sum(delta) <> 0
          ORDER BY account_id, instrument_id",
    )
    .bind(owner_id)
    .bind(on)
    .fetch_all(pool)
    .await
    .context("adding up positions")
}

/// A revision as it is stored.
#[derive(sqlx::FromRow)]
struct RevisionRow {
    id: i64,
    action: RevisionAction,
    actor: WhoKind,
    actor_person: Option<Uuid>,
    actor_module: Option<String>,
    at: DateTime<Utc>,
    before: Option<Json<TradeState>>,
}

/// Everything that happened to a trade, oldest first; `None` when the person
/// has no trade by that id and never had.
///
/// # Errors
///
/// Returns an error when a query fails or a stored state does not read.
pub async fn history(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Vec<TradeRevision>>> {
    let rows: Vec<RevisionRow> = sqlx::query_as(
        "SELECT id, action, actor, actor_person, actor_module, at, before
           FROM trade_revisions WHERE owner_id = $1 AND trade_id = $2
          ORDER BY id",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_all(pool)
    .await
    .context("reading a trade's history")?;
    let now: Option<Json<TradeState>> = sqlx::query_scalar("SELECT trade_snapshot(id) FROM trades WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading what a trade is now")?;
    if rows.is_empty() && now.is_none() {
        return Ok(None);
    }

    // What a trade was after one revision is what it was before the next, or
    // what it is now.
    let mut afters: Vec<Option<TradeState>> = rows.iter().skip(1).map(|row| row.before.as_ref().map(|json| json.0.clone())).collect();
    afters.push(now.map(|json| json.0));
    Ok(Some(
        rows.into_iter()
            .zip(afters)
            .map(|(row, after)| TradeRevision {
                id: row.id,
                at: row.at,
                action: row.action,
                who: Who {
                    kind: row.actor,
                    person_id: row.actor_person,
                    module: row.actor_module,
                },
                before: row.before.map(|json| json.0),
                after: if row.action == RevisionAction::Deleted { None } else { after },
            })
            .collect(),
    ))
}

/// Keeps a valuation as a snapshot.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn save_snapshot(pool: &PgPool, owner_id: Uuid, valuation: &Valuation, note: &str) -> Result<Snapshot> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    let (id, taken_at): (Uuid, DateTime<Utc>) =
        sqlx::query_as("INSERT INTO snapshots (owner_id, on_date, priced_at, currency, note) VALUES ($1, $2, $3, $4, $5) RETURNING id, taken_at")
            .bind(owner_id)
            .bind(valuation.on)
            .bind(valuation.at)
            .bind(&valuation.currency)
            .bind(note)
            .fetch_one(&mut *transaction)
            .await
            .context("taking a snapshot")?;
    for account in &valuation.accounts {
        for holding in &account.holdings {
            sqlx::query(
                "INSERT INTO snapshot_lines (snapshot_id, kind, account_id, account_name, instrument_id, symbol, name, quantity, price, priced_at, value, stale)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(id)
            .bind(holding.kind)
            .bind(account.account_id)
            .bind(&account.name)
            .bind(holding.instrument_id)
            .bind(&holding.symbol)
            .bind(&holding.name)
            .bind(holding.quantity)
            .bind(holding.price)
            .bind(holding.priced_at)
            .bind(holding.value)
            .bind(holding.stale)
            .execute(&mut *transaction)
            .await
            .context("keeping a line of a snapshot")?;
        }
    }
    transaction.commit().await.context("taking a snapshot")?;
    Ok(Snapshot {
        id,
        note: note.to_owned(),
        taken_at,
        valuation: valuation.clone(),
    })
}

/// A person's snapshots, the latest moment first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn snapshots(pool: &PgPool, owner_id: Uuid) -> Result<Vec<SnapshotSummary>> {
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT s.id, s.on_date, s.currency, s.note, s.taken_at,
                (SELECT sum(l.value) FROM snapshot_lines l WHERE l.snapshot_id = s.id) AS total,
                EXISTS (SELECT 1 FROM snapshot_lines l WHERE l.snapshot_id = s.id AND l.value IS NULL) AS incomplete,
                EXISTS (SELECT 1 FROM snapshot_lines l WHERE l.snapshot_id = s.id AND l.value IS NOT NULL AND l.stale) AS stale
           FROM snapshots s
          WHERE s.owner_id = $1
          ORDER BY s.on_date DESC, s.taken_at DESC",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing snapshots")?;
    Ok(rows
        .into_iter()
        .map(|row| SnapshotSummary {
            id: row.id,
            on: row.on_date,
            currency: row.currency,
            note: row.note,
            taken_at: row.taken_at,
            total: row.total.unwrap_or_default().normalize(),
            incomplete: row.incomplete,
            stale: row.stale,
        })
        .collect())
}

/// A snapshot in the list, as it is read.
#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: Uuid,
    on_date: NaiveDate,
    currency: String,
    note: String,
    taken_at: DateTime<Utc>,
    total: Option<Decimal>,
    incomplete: bool,
    stale: bool,
}

/// A line of a snapshot as it is stored.
#[derive(sqlx::FromRow)]
struct SnapshotLine {
    kind: LineKind,
    account_id: Uuid,
    account_name: String,
    instrument_id: Option<Uuid>,
    symbol: String,
    name: String,
    quantity: Decimal,
    price: Option<Decimal>,
    priced_at: Option<DateTime<Utc>>,
    value: Option<Decimal>,
    stale: bool,
}

/// One snapshot of this person's, as it was taken.
///
/// # Errors
///
/// Returns an error when a query fails.
pub async fn snapshot(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Snapshot>> {
    let Some(header): Option<SnapshotHeader> =
        sqlx::query_as("SELECT on_date, priced_at, currency, note, taken_at FROM snapshots WHERE owner_id = $1 AND id = $2")
            .bind(owner_id)
            .bind(id)
            .fetch_optional(pool)
            .await
            .context("reading a snapshot")?
    else {
        return Ok(None);
    };
    let lines: Vec<SnapshotLine> = sqlx::query_as(
        "SELECT kind, account_id, account_name, instrument_id, symbol, name, quantity, price, priced_at, value, stale
           FROM snapshot_lines WHERE snapshot_id = $1",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .context("reading the lines of a snapshot")?;

    let mut accounts: BTreeMap<Uuid, AccountValue> = BTreeMap::new();
    for line in lines {
        let account = accounts.entry(line.account_id).or_insert_with(|| AccountValue {
            account_id: line.account_id,
            name: line.account_name.clone(),
            holdings: Vec::new(),
            total: Decimal::ZERO,
        });
        account.holdings.push(Holding {
            kind: line.kind,
            instrument_id: line.instrument_id,
            symbol: line.symbol,
            name: line.name,
            quantity: line.quantity.normalize(),
            price: line.price.map(|price| price.normalize()),
            priced_at: line.priced_at,
            value: line.value.map(|value| value.normalize()),
            stale: line.stale,
        });
    }
    let valuation = crate::value::assemble(header.on_date, header.priced_at, header.currency, accounts.into_values().collect());
    Ok(Some(Snapshot {
        id,
        note: header.note,
        taken_at: header.taken_at,
        valuation,
    }))
}

/// A snapshot's own columns, as they are read.
#[derive(sqlx::FromRow)]
struct SnapshotHeader {
    on_date: NaiveDate,
    priced_at: DateTime<Utc>,
    currency: String,
    note: String,
    taken_at: DateTime<Utc>,
}

/// Deletes a snapshot.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn delete_snapshot(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM snapshots WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("deleting a snapshot")?;
    Ok(deleted.rows_affected() > 0)
}
