//! Files kept with entries.

use anyhow::{Context, Result};
use austeris_common::Actor;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::Attachment;

/// What attaching a file came to.
#[derive(Debug)]
pub enum Attached {
    /// It is on the entry.
    Now(Attachment),
    /// There is no entry of this person's in the books by that id.
    NoEntry,
    /// The same file is on the entry already: one receipt, uploaded twice.
    Already(Attachment),
}

/// Keeps a file with an entry.
///
/// `media_type` is what the bytes were read as ([`crate::attachment::media_type`]);
/// the caller has already refused anything else.
///
/// # Errors
///
/// Returns an error when a write fails.
pub async fn attach(pool: &PgPool, owner_id: Uuid, by: &Actor, entry_id: Uuid, name: &str, media_type: &str, content: &[u8]) -> Result<Attached> {
    let sha256 = Sha256::digest(content).to_vec();
    let size = i64::try_from(content.len()).context("a file larger than the database can say")?;

    let mut transaction = by.begin(pool).await?;
    // Locked, so a deletion of the entry waits for the file to be on it - and
    // takes the file to the trash with it - rather than racing past it.
    let live: Option<Uuid> = sqlx::query_scalar("SELECT id FROM entries WHERE owner_id = $1 AND id = $2 FOR UPDATE")
        .bind(owner_id)
        .bind(entry_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("finding the entry")?;
    if live.is_none() {
        return Ok(Attached::NoEntry);
    }
    let twin: Option<Attachment> = sqlx::query_as(
        "SELECT id, name, media_type, size, created_at FROM attachments
          WHERE entry_id = $1 AND sha256 = $2 AND removed_at IS NULL",
    )
    .bind(entry_id)
    .bind(&sha256)
    .fetch_optional(&mut *transaction)
    .await
    .context("looking for the same file on the entry")?;
    if let Some(twin) = twin {
        return Ok(Attached::Already(twin));
    }

    let attachment: Attachment = sqlx::query_as(
        "INSERT INTO attachments (owner_id, entry_id, name, media_type, size, sha256, content)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, name, media_type, size, created_at",
    )
    .bind(owner_id)
    .bind(entry_id)
    .bind(name)
    .bind(media_type)
    .bind(size)
    .bind(&sha256)
    .bind(content)
    .fetch_one(&mut *transaction)
    .await
    .context("keeping the file")?;
    transaction.commit().await.context("keeping the file")?;
    Ok(Attached::Now(attachment))
}

/// A file of this person's and its bytes - on an entry, taken off one, or
/// waiting in the trash with its entry. Gone only once it is erased.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn attachment_content(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<(Attachment, Vec<u8>)>> {
    let stored: Option<Stored> = sqlx::query_as(
        "SELECT id, name, media_type, size, created_at, content FROM attachments
          WHERE owner_id = $1 AND id = $2",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading a file")?;
    Ok(stored.map(|stored| (stored.attachment, stored.content)))
}

/// A file as it is stored: what it is, and its bytes.
#[derive(sqlx::FromRow)]
struct Stored {
    #[sqlx(flatten)]
    attachment: Attachment,
    content: Vec<u8>,
}

/// Takes a file off its entry, to the trash.
///
/// Only a file on an entry in the books: the files of an entry in the trash go
/// back with it, or are erased with it.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn remove_attachment(pool: &PgPool, owner_id: Uuid, by: &Actor, id: Uuid) -> Result<bool> {
    let mut transaction = by.begin(pool).await?;
    let removed = sqlx::query(
        "UPDATE attachments a SET removed_at = now()
          WHERE a.owner_id = $1 AND a.id = $2 AND a.removed_at IS NULL
            AND EXISTS (SELECT 1 FROM entries e WHERE e.id = a.entry_id)",
    )
    .bind(owner_id)
    .bind(id)
    .execute(&mut *transaction)
    .await
    .context("taking a file off its entry")?;
    transaction.commit().await.context("taking a file off its entry")?;
    Ok(removed.rows_affected() > 0)
}
