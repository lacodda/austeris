//! The trash: deleted entries and files taken off entries, for as long as it
//! keeps them.
//!
//! An entry in the trash is not a row anywhere a balance reads: its history's
//! last word is its deletion, and what it was is that revision's snapshot.
//! Restoring writes the snapshot back under the same ids. Erasing - by hand, or
//! when the trash has kept it long enough - removes the entry's history and
//! its files with it, and leaves nothing to restore.

use std::collections::HashSet;

use anyhow::{Context, Result};
use austeris_common::Actor;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::details::{choose_counterparty, find_or_create_place, set_tags};
use super::history::{DeletionRow, Snapshot, deletion_for_update, deletions_select};
use super::{CounterpartyChoice, NewLine, NewPlace, Side, ensure_sides_belong_to, entry};
use crate::model::{Attachment, Entry, LineSide, Trash, TrashedAttachment, TrashedEntry};
use crate::trash::KEEP_DAYS;

/// Makes a person's trash wait for whatever else is being done to it.
///
/// Two presses of Restore arrive as two requests at once, and a lock on the
/// row each reads is not enough: the first one writes the entry back without
/// changing that row, so the second would read it as still deleted and write
/// the entry a second time. Taken first, before anything is read, this makes
/// the second read what the first left.
async fn lock_trash(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ledger.trash'), hashtext($1::text))")
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

/// What is in a person's trash.
///
/// # Errors
///
/// Returns an error when a query fails or a stored snapshot does not read.
pub async fn trash(pool: &PgPool, owner_id: Uuid) -> Result<Trash> {
    let deleted: Vec<DeletionRow> = sqlx::query_as(concat!(deletions_select!(), " ORDER BY r.at DESC, r.id DESC"))
        .bind(owner_id)
        .bind(KEEP_DAYS)
        .fetch_all(pool)
        .await
        .context("reading the trash")?;
    let entries = deleted
        .into_iter()
        .map(|row| {
            let (id, deletion) = row.into_deletion();
            TrashedEntry {
                entry: deletion.before.into_entry(id),
                deleted_at: deletion.at,
                who: deletion.who,
                erased_after: erased_after(deletion.at),
            }
        })
        .collect();

    let removed: Vec<RemovedRow> = sqlx::query_as(
        "SELECT a.id, a.name, a.media_type, a.size, a.created_at, a.removed_at,
                e.id AS entry_id, e.occurred_on, e.description, cp.name AS counterparty_name
           FROM attachments a
           JOIN entries e ON e.id = a.entry_id
           LEFT JOIN counterparties cp ON cp.id = e.counterparty_id
          WHERE a.owner_id = $1 AND a.removed_at IS NOT NULL
            AND a.removed_at > now() - make_interval(days => $2)
          ORDER BY a.removed_at DESC, a.id DESC",
    )
    .bind(owner_id)
    .bind(KEEP_DAYS)
    .fetch_all(pool)
    .await
    .context("reading the files in the trash")?;
    let attachments = removed.into_iter().map(RemovedRow::into_trashed).collect();

    Ok(Trash {
        keep_days: KEEP_DAYS,
        entries,
        attachments,
    })
}

/// A file taken off an entry, as it is read.
#[derive(sqlx::FromRow)]
struct RemovedRow {
    id: Uuid,
    name: String,
    media_type: String,
    size: i64,
    created_at: DateTime<Utc>,
    removed_at: DateTime<Utc>,
    entry_id: Uuid,
    occurred_on: NaiveDate,
    description: String,
    counterparty_name: Option<String>,
}

impl RemovedRow {
    fn into_trashed(self) -> TrashedAttachment {
        let entry_title = match (self.description.as_str(), self.counterparty_name) {
            ("", Some(name)) => name,
            (description, _) => description.to_owned(),
        };
        TrashedAttachment {
            attachment: Attachment {
                id: self.id,
                name: self.name,
                media_type: self.media_type,
                size: self.size,
                created_at: self.created_at,
            },
            entry_id: self.entry_id,
            entry_title,
            entry_occurred_on: self.occurred_on,
            removed_at: self.removed_at,
            erased_after: erased_after(self.removed_at),
        }
    }
}

/// What restoring an entry came to.
#[derive(Debug)]
pub enum Restored {
    /// It is back in the books.
    Back(Box<Entry>),
    /// There is no entry of this person's in the trash by that id.
    Missing,
}

/// Refused: the entry cannot come back as it was.
#[derive(Debug, thiserror::Error)]
pub enum RestoreRefused {
    /// It moved money on an account or under a category that is gone.
    #[error("the entry moved money on an account or under a category that no longer exists, so it cannot come back as it was")]
    SideGone,
    /// A module recorded the same movement again since, under the same key.
    #[error("the module that posted this entry has posted it again since; the newer one stands")]
    PostedAgain,
}

/// Brings an entry back from the trash, as it was deleted.
///
/// Under the same id, with the same lines, the day it was first recorded and
/// the key a module posted it under. Its counterparty, place and tags are the
/// ones it had, found by id - or, when one was deleted while the entry was in
/// the trash, found or made again by name, the way typing it would. Its files
/// waited for it and are on it again.
///
/// # Errors
///
/// Returns [`RestoreRefused`] when it cannot come back as it was, or an error
/// when a write fails.
pub async fn restore_entry(pool: &PgPool, owner_id: Uuid, by: &Actor, id: Uuid) -> Result<Restored> {
    let mut transaction = by.begin(pool).await?;
    lock_trash(&mut transaction, owner_id).await?;
    let Some(deletion) = deletion_for_update(&mut transaction, owner_id, id, KEEP_DAYS).await? else {
        return Ok(Restored::Missing);
    };
    let was = deletion.before;

    let counterparty_id = match &was.counterparty {
        Some(counterparty) if still_there(&mut transaction, "counterparties", owner_id, counterparty.id).await? => Some(counterparty.id),
        Some(counterparty) => {
            let choice = CounterpartyChoice::New {
                name: counterparty.name.clone(),
                usual_category: None,
                tax_id: None,
            };
            Some(choose_counterparty(&mut transaction, owner_id, &choice).await?)
        }
        None => None,
    };
    let place_id = match &was.place {
        Some(place) if still_there(&mut transaction, "places", owner_id, place.id).await? => Some(place.id),
        Some(place) => {
            let again = NewPlace {
                country: place.country.clone(),
                city: place.city.clone(),
            };
            Some(find_or_create_place(&mut transaction, owner_id, &again).await?)
        }
        None => None,
    };

    let sides = sides_of(&was, id)?;
    // A side that is gone is the refusal; a query that failed is a fault.
    if let Err(error) = ensure_sides_belong_to(&mut transaction, owner_id, &sides).await {
        let failed = error.chain().any(|cause| cause.downcast_ref::<sqlx::Error>().is_some());
        return Err(if failed { error } else { RestoreRefused::SideGone.into() });
    }

    let written = sqlx::query(
        "INSERT INTO entries (id, owner_id, occurred_on, description, idempotency_key, source, counterparty_id, place_id, status, cleared_on, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(owner_id)
    .bind(was.occurred_on)
    .bind(&was.description)
    .bind(was.idempotency_key.as_deref())
    .bind(was.source.as_deref())
    .bind(counterparty_id)
    .bind(place_id)
    .bind(was.status)
    .bind(was.cleared_on)
    .bind(was.created_at)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = written {
        let posted_again = error.as_database_error().is_some_and(|db| db.code().as_deref() == Some("23505"));
        return Err(if posted_again {
            RestoreRefused::PostedAgain.into()
        } else {
            anyhow::Error::new(error).context("writing the entry back")
        });
    }

    for (line, side) in was.lines.iter().zip(&sides) {
        sqlx::query(
            "INSERT INTO entry_lines (id, entry_id, side, account_id, category_id, amount, currency, note)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(line.id)
        .bind(id)
        .bind(side.side.kind())
        .bind(side.side.account())
        .bind(side.side.category())
        .bind(line.amount)
        .bind(&line.currency)
        .bind(&line.note)
        .execute(&mut *transaction)
        .await
        .context("writing a line back")?;
    }
    tag_again(&mut transaction, owner_id, id, &was).await?;

    transaction.commit().await.context("restoring the entry")?;
    Ok(entry(pool, owner_id, id)
        .await?
        .map_or(Restored::Missing, |entry| Restored::Back(Box::new(entry))))
}

/// Tags an entry written back with what it carried: a tag is the one it was
/// while it exists - renamed since or not - and is made again by its name when
/// it was deleted.
async fn tag_again(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid, was: &Snapshot) -> Result<()> {
    let wanted: Vec<Uuid> = was.tags.iter().map(|tag| tag.id).collect();
    let kept: HashSet<Uuid> = sqlx::query_scalar("SELECT id FROM tags WHERE owner_id = $1 AND id = ANY($2)")
        .bind(owner_id)
        .bind(&wanted)
        .fetch_all(&mut **transaction)
        .await
        .context("finding the entry's tags")?
        .into_iter()
        .collect();
    sqlx::query("INSERT INTO entry_tags (owner_id, entry_id, tag_id) SELECT $1, $2, unnest($3::uuid[])")
        .bind(owner_id)
        .bind(id)
        .bind(kept.iter().copied().collect::<Vec<_>>())
        .execute(&mut **transaction)
        .await
        .context("tagging the entry again")?;
    let gone: Vec<String> = was.tags.iter().filter(|tag| !kept.contains(&tag.id)).map(|tag| tag.name.clone()).collect();
    set_tags(transaction, owner_id, id, &gone).await
}

/// The sides of an entry as it was, to be checked and written again.
fn sides_of(was: &Snapshot, id: Uuid) -> Result<Vec<NewLine>> {
    was.lines
        .iter()
        .map(|line| {
            let side = match (line.side, line.account_id, line.category_id) {
                (LineSide::Account, Some(account), _) => Side::Account(account),
                (LineSide::Category, _, Some(category)) => Side::Category(category),
                (LineSide::Conversion, ..) => Side::Conversion,
                _ => anyhow::bail!("a line of entry {id} in the trash names neither its account nor its category"),
            };
            Ok(NewLine {
                side,
                amount: line.amount,
                currency: line.currency.clone(),
                note: line.note.clone(),
            })
        })
        .collect()
}

/// Whether a row of one of the entry's own tables is still this person's.
async fn still_there(transaction: &mut Transaction<'_, Postgres>, table: &'static str, owner_id: Uuid, id: Uuid) -> Result<bool> {
    // Audited: `table` is one of two literals in this file.
    let found: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {table} WHERE owner_id = $1 AND id = $2")))
        .bind(owner_id)
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .with_context(|| format!("looking for the entry's row in {table}"))?;
    Ok(found.is_some())
}

/// Erases an entry in the trash for good: its history and its files.
///
/// Opened without saying who is writing, on purpose: erasing touches no entry
/// in the books, and if a fault ever pointed it at one, the database would
/// refuse a write to that entry's files that names nobody.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn erase_entry(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    lock_trash(&mut transaction, owner_id).await?;
    if deletion_for_update(&mut transaction, owner_id, id, KEEP_DAYS).await?.is_none() {
        return Ok(false);
    }
    erase_entries(&mut transaction, owner_id, &[id]).await?;
    transaction.commit().await.context("erasing the entry")?;
    Ok(true)
}

/// Removes every trace of entries that are in the trash.
async fn erase_entries(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, ids: &[Uuid]) -> Result<()> {
    sqlx::query("DELETE FROM attachments WHERE owner_id = $1 AND entry_id = ANY($2)")
        .bind(owner_id)
        .bind(ids)
        .execute(&mut **transaction)
        .await
        .context("erasing the entry's files")?;
    sqlx::query("DELETE FROM entry_revisions WHERE owner_id = $1 AND entry_id = ANY($2)")
        .bind(owner_id)
        .bind(ids)
        .execute(&mut **transaction)
        .await
        .context("erasing the entry's history")?;
    Ok(())
}

/// What restoring a file came to.
#[derive(Debug)]
pub enum FileRestored {
    /// It is on its entry again.
    Back(Attachment),
    /// There is no file of this person's in the trash by that id.
    Missing,
    /// Its entry is in the trash too; the file comes back with it.
    EntryInTrash,
    /// The same file has been attached to the entry again since.
    AttachedAgain,
}

/// Puts a file taken off an entry back on it.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn restore_attachment(pool: &PgPool, owner_id: Uuid, by: &Actor, id: Uuid) -> Result<FileRestored> {
    let mut transaction = by.begin(pool).await?;
    lock_trash(&mut transaction, owner_id).await?;
    let found: Option<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT entry_id, sha256 FROM attachments
          WHERE owner_id = $1 AND id = $2 AND removed_at IS NOT NULL AND removed_at > now() - make_interval(days => $3)
          FOR UPDATE",
    )
    .bind(owner_id)
    .bind(id)
    .bind(KEEP_DAYS)
    .fetch_optional(&mut *transaction)
    .await
    .context("finding the file in the trash")?;
    let Some((entry_id, sha256)) = found else {
        return Ok(FileRestored::Missing);
    };

    let live: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM entries WHERE owner_id = $1 AND id = $2)")
        .bind(owner_id)
        .bind(entry_id)
        .fetch_one(&mut *transaction)
        .await
        .context("finding the file's entry")?;
    if !live {
        return Ok(FileRestored::EntryInTrash);
    }
    let twin: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM attachments WHERE entry_id = $1 AND sha256 = $2 AND removed_at IS NULL)")
        .bind(entry_id)
        .bind(&sha256)
        .fetch_one(&mut *transaction)
        .await
        .context("looking for the same file on the entry")?;
    if twin {
        return Ok(FileRestored::AttachedAgain);
    }

    let attachment: Attachment = sqlx::query_as(
        "UPDATE attachments SET removed_at = NULL WHERE owner_id = $1 AND id = $2
         RETURNING id, name, media_type, size, created_at",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_one(&mut *transaction)
    .await
    .context("putting the file back")?;
    transaction.commit().await.context("putting the file back")?;
    Ok(FileRestored::Back(attachment))
}

/// Erases a file taken off an entry, for good.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn erase_attachment(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let erased = sqlx::query("DELETE FROM attachments WHERE owner_id = $1 AND id = $2 AND removed_at IS NOT NULL")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("erasing a file")?;
    Ok(erased.rows_affected() > 0)
}

/// Erases everything in a person's trash.
///
/// # Errors
///
/// Returns an error when a query or a write fails.
pub async fn empty_trash(pool: &PgPool, owner_id: Uuid) -> Result<()> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    lock_trash(&mut transaction, owner_id).await?;
    let deleted: Vec<DeletionRow> = sqlx::query_as(concat!(deletions_select!(), " FOR UPDATE OF r"))
        .bind(owner_id)
        .bind(KEEP_DAYS)
        .fetch_all(&mut *transaction)
        .await
        .context("reading the trash")?;
    let ids: Vec<Uuid> = deleted.iter().map(|row| row.entry_id).collect();
    erase_entries(&mut transaction, owner_id, &ids).await?;
    sqlx::query("DELETE FROM attachments WHERE owner_id = $1 AND removed_at IS NOT NULL")
        .bind(owner_id)
        .execute(&mut *transaction)
        .await
        .context("erasing the files in the trash")?;
    transaction.commit().await.context("emptying the trash")?;
    Ok(())
}

/// What erasing what the trash has kept long enough erased.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Swept {
    /// Entries, each with its history and files.
    pub entries: i64,
    /// Files: those of the entries, and those taken off entries.
    pub attachments: i64,
}

/// Erases everything that has been in anyone's trash for longer than it
/// keeps things.
///
/// Every person's at once: this is the installation's housekeeping, not a
/// request of anyone's.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn sweep(pool: &PgPool) -> Result<Swept> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    // The entry's absence from the books is checked as well as its history's
    // last word: a sweep that ever reached a live entry would erase its
    // receipts, and two conditions are harder to get wrong at once than one.
    let (entries, files): (i64, i64) = sqlx::query_as(
        "WITH expired AS (
             SELECT r.entry_id FROM entry_revisions r
              WHERE r.action = 'deleted' AND r.at <= now() - make_interval(days => $1)
                AND r.id = (SELECT max(x.id) FROM entry_revisions x WHERE x.entry_id = r.entry_id)
                AND NOT EXISTS (SELECT 1 FROM entries e WHERE e.id = r.entry_id)
         ),
         files AS (DELETE FROM attachments a USING expired x WHERE a.entry_id = x.entry_id RETURNING a.id),
         -- Run to completion whether or not anything reads it, as every
         -- statement that writes in a WITH is.
         history AS (DELETE FROM entry_revisions r USING expired x WHERE r.entry_id = x.entry_id)
         SELECT (SELECT count(*) FROM expired), (SELECT count(*) FROM files)",
    )
    .bind(KEEP_DAYS)
    .fetch_one(&mut *transaction)
    .await
    .context("erasing the entries the trash has kept long enough")?;
    let removed = sqlx::query("DELETE FROM attachments WHERE removed_at IS NOT NULL AND removed_at <= now() - make_interval(days => $1)")
        .bind(KEEP_DAYS)
        .execute(&mut *transaction)
        .await
        .context("erasing the files the trash has kept long enough")?
        .rows_affected();
    transaction.commit().await.context("emptying what the trash has kept long enough")?;
    Ok(Swept {
        entries,
        attachments: files + i64::try_from(removed).unwrap_or(i64::MAX),
    })
}
