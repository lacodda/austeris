//! The trash: deleted trades, for as long as it keeps them.
//!
//! A trade in the trash is not a row any position reads: its history's last
//! word is its deletion, and what it was is that revision's snapshot - the
//! ledger keeps its entries' trash the same way (ADR 0013). Restoring writes
//! the trade back under its own id and books its money again under the key it
//! was booked under (`trade::restore`); erasing - by hand, or once the trash
//! has kept it long enough - removes its history, and leaves nothing to
//! restore.

use std::time::Duration as Every;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub use austeris_common::trash::KEEP_DAYS;

use crate::model::{Trade, TradeKind, TradeTrash, TrashedTrade, Who, WhoKind};
use crate::trade::with_money;

/// How often the trash is looked through.
const EVERY: Every = Every::from_hours(1);

/// Makes a person's trash wait for whatever else is being done to it.
///
/// Two presses of Restore arrive as two requests at once, and a lock on the
/// revision each reads is not enough: the first writes the trade back without
/// changing that row, so the second would read the trade as still deleted and
/// write it a second time. Taken first, before anything is read, this makes
/// the second read what the first left.
///
/// # Errors
///
/// Returns an error when the lock cannot be taken.
pub async fn lock(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('portfolio.trash'), hashtext($1::text))")
        .bind(owner_id)
        .execute(&mut **transaction)
        .await
        .context("waiting for this person's trash")?;
    Ok(())
}

/// When something put in the trash at `at` is erased.
fn erased_after(at: DateTime<Utc>) -> DateTime<Utc> {
    at + Duration::days(i64::from(KEEP_DAYS))
}

/// The deletions still in a person's trash: each the last word of its trade's
/// history, with no trade by its id in the portfolio, and younger than the
/// trash keeps things. The trade is read out of the snapshot the deletion
/// kept; a snapshot taken before trades remembered their key and their
/// recording reads as recorded when it was deleted, with no key.
macro_rules! deletions_select {
    () => {
        "SELECT r.trade_id AS id,
                (r.before->>'kind')::trade_kind AS kind,
                (r.before->>'account_id')::uuid AS account_id,
                (r.before->>'to_account_id')::uuid AS to_account_id,
                (r.before->>'instrument_id')::uuid AS instrument_id,
                (r.before->>'quantity')::numeric AS quantity,
                (r.before->>'price')::numeric AS price,
                r.before->>'currency' AS currency,
                (r.before->>'fee')::numeric AS fee,
                (r.before->>'occurred_at')::timestamptz AS occurred_at,
                (r.before->>'occurred_on')::date AS occurred_on,
                COALESCE(r.before->>'note', '') AS note,
                r.before->>'idempotency_key' AS idempotency_key,
                COALESCE((r.before->>'created_at')::timestamptz, r.at) AS created_at,
                r.at AS deleted_at, r.actor, r.actor_person, r.actor_module
           FROM trade_revisions r
          WHERE r.owner_id = $1 AND r.action = 'deleted'
            AND r.at > now() - make_interval(days => $2)
            AND r.id = (SELECT max(x.id) FROM trade_revisions x WHERE x.trade_id = r.trade_id)
            AND NOT EXISTS (SELECT 1 FROM trades t WHERE t.id = r.trade_id)"
    };
}

/// A deletion, as it is read.
#[derive(sqlx::FromRow)]
struct DeletionRow {
    id: Uuid,
    kind: TradeKind,
    account_id: Uuid,
    to_account_id: Option<Uuid>,
    instrument_id: Uuid,
    quantity: Decimal,
    price: Option<Decimal>,
    currency: Option<String>,
    fee: Decimal,
    occurred_at: DateTime<Utc>,
    occurred_on: NaiveDate,
    note: String,
    idempotency_key: Option<String>,
    created_at: DateTime<Utc>,
    deleted_at: DateTime<Utc>,
    actor: WhoKind,
    actor_person: Option<Uuid>,
    actor_module: Option<String>,
}

impl DeletionRow {
    fn into_trashed(self) -> TrashedTrade {
        TrashedTrade {
            trade: with_money(Trade {
                id: self.id,
                kind: self.kind,
                account_id: self.account_id,
                to_account_id: self.to_account_id,
                instrument_id: self.instrument_id,
                quantity: self.quantity,
                price: self.price,
                currency: self.currency,
                fee: self.fee,
                occurred_at: self.occurred_at,
                occurred_on: self.occurred_on,
                note: self.note,
                entry_id: None,
                money: None,
                idempotency_key: self.idempotency_key,
                created_at: self.created_at,
                updated_at: self.deleted_at,
            }),
            deleted_at: self.deleted_at,
            who: Who {
                kind: self.actor,
                person_id: self.actor_person,
                module: self.actor_module,
            },
            erased_after: erased_after(self.deleted_at),
        }
    }
}

/// What is in a person's trash.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn trash(pool: &PgPool, owner_id: Uuid) -> Result<TradeTrash> {
    let rows: Vec<DeletionRow> = sqlx::query_as(concat!(deletions_select!(), " ORDER BY r.at DESC, r.id DESC"))
        .bind(owner_id)
        .bind(KEEP_DAYS)
        .fetch_all(pool)
        .await
        .context("reading the trash")?;
    Ok(TradeTrash {
        keep_days: KEEP_DAYS,
        trades: rows.into_iter().map(DeletionRow::into_trashed).collect(),
    })
}

/// The trade in the trash by this id, with its deletion locked until the
/// transaction ends. Take [`lock`] first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn trashed_for_update(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid) -> Result<Option<TrashedTrade>> {
    let row: Option<DeletionRow> = sqlx::query_as(concat!(deletions_select!(), " AND r.trade_id = $3 FOR UPDATE OF r"))
        .bind(owner_id)
        .bind(KEEP_DAYS)
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .context("finding the trade in the trash")?;
    Ok(row.map(DeletionRow::into_trashed))
}

/// Writes a trade back from the trash, as it was: its id, its key, when it
/// was first recorded. Its money is the caller's to book again.
///
/// # Errors
///
/// Returns an error when a rule of the schema refuses it - a position below
/// nothing, a key another trade has taken since - or the write fails.
pub async fn write_back(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, trade: &Trade) -> Result<Trade> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO trades (id, owner_id, kind, account_id, to_account_id, instrument_id, quantity, price, currency, fee,
                             occurred_at, occurred_on, note, idempotency_key, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         RETURNING {}",
        crate::repository::TRADE_COLUMNS
    )))
    .bind(trade.id)
    .bind(owner_id)
    .bind(trade.kind)
    .bind(trade.account_id)
    .bind(trade.to_account_id)
    .bind(trade.instrument_id)
    .bind(trade.quantity)
    .bind(trade.price)
    .bind(trade.currency.as_deref())
    .bind(trade.fee)
    .bind(trade.occurred_at)
    .bind(trade.occurred_on)
    .bind(&trade.note)
    .bind(trade.idempotency_key.as_deref())
    .bind(trade.created_at)
    .fetch_one(&mut **transaction)
    .await
    .context("writing a trade back from the trash")
}

/// Erases one trade in the trash: its history goes, and it cannot come back.
/// False when there is no such trade in the person's trash.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn erase(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    lock(&mut transaction, owner_id).await?;
    if trashed_for_update(&mut transaction, owner_id, id).await?.is_none() {
        return Ok(false);
    }
    sqlx::query("DELETE FROM trade_revisions WHERE owner_id = $1 AND trade_id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(&mut *transaction)
        .await
        .context("erasing a trade's history")?;
    transaction.commit().await.context("erasing a trade")?;
    Ok(true)
}

/// Erases everything in a person's trash. Answers how many trades it erased.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn empty(pool: &PgPool, owner_id: Uuid) -> Result<u64> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    lock(&mut transaction, owner_id).await?;
    let erased: i64 = sqlx::query_scalar(
        "WITH trashed AS (
             SELECT r.trade_id FROM trade_revisions r
              WHERE r.owner_id = $1 AND r.action = 'deleted'
                AND r.id = (SELECT max(x.id) FROM trade_revisions x WHERE x.trade_id = r.trade_id)
                AND NOT EXISTS (SELECT 1 FROM trades t WHERE t.id = r.trade_id)
         ),
         history AS (DELETE FROM trade_revisions r USING trashed x WHERE r.trade_id = x.trade_id)
         SELECT count(*) FROM trashed",
    )
    .bind(owner_id)
    .fetch_one(&mut *transaction)
    .await
    .context("emptying the trash")?;
    transaction.commit().await.context("emptying the trash")?;
    Ok(u64::try_from(erased).unwrap_or_default())
}

/// Erases every trade that has been in anyone's trash longer than it keeps
/// things. Answers how many.
///
/// Every person's at once: this is the installation's housekeeping, not a
/// request of anyone's. The trade's absence is checked as well as its
/// history's last word: a sweep that ever reached a live trade would erase
/// what it was, and two conditions are harder to get wrong at once than one.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn sweep(pool: &PgPool) -> Result<u64> {
    let erased: i64 = sqlx::query_scalar(
        "WITH expired AS (
             SELECT r.trade_id FROM trade_revisions r
              WHERE r.action = 'deleted' AND r.at <= now() - make_interval(days => $1)
                AND r.id = (SELECT max(x.id) FROM trade_revisions x WHERE x.trade_id = r.trade_id)
                AND NOT EXISTS (SELECT 1 FROM trades t WHERE t.id = r.trade_id)
         ),
         history AS (DELETE FROM trade_revisions r USING expired x WHERE r.trade_id = x.trade_id)
         SELECT count(*) FROM expired",
    )
    .bind(KEEP_DAYS)
    .fetch_one(pool)
    .await
    .context("erasing the trades the trash has kept long enough")?;
    Ok(u64::try_from(erased).unwrap_or_default())
}

/// Erases, every hour, what has been in the trash longer than [`KEEP_DAYS`].
///
/// Hourly rather than when someone looks: "erased after thirty days" is a
/// promise about what is kept, not about what is shown.
pub async fn every_hour(pool: PgPool) {
    let mut ticks = tokio::time::interval(EVERY);
    loop {
        ticks.tick().await;
        match sweep(&pool).await {
            Ok(0) => {}
            Ok(trades) => tracing::info!(trades, "erased the trades the trash kept long enough"),
            Err(error) => tracing::error!(error = format!("{error:#}"), "erasing the portfolio's trash failed"),
        }
    }
}
