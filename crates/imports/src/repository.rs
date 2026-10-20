//! Reading and writing this service's schema.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::types::Json;
use uuid::Uuid;

use crate::model::{Document, Needed, Origin, Reason, State};
use crate::sifen::Receipt;

/// The columns a document is read with - everything but its file.
const COLUMNS: &str = "id, name, size, origin, state, receipt, account_id, category_id, account_name, needs, reason, entry_id, received_at, booked_at";

/// A document as it is stored.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Stored {
    id: Uuid,
    name: String,
    size: i64,
    origin: Origin,
    state: State,
    receipt: Option<Json<Receipt>>,
    account_id: Option<Uuid>,
    category_id: Option<Uuid>,
    account_name: Option<String>,
    needs: Vec<String>,
    reason: Option<Json<Reason>>,
    entry_id: Option<Uuid>,
    received_at: DateTime<Utc>,
    booked_at: Option<DateTime<Utc>>,
}

impl Stored {
    /// Which one.
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Where it stands.
    #[must_use]
    pub fn state(&self) -> State {
        self.state
    }

    /// The file's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What it was read as.
    #[must_use]
    pub fn receipt(&self) -> Option<&Receipt> {
        self.receipt.as_ref().map(|receipt| &receipt.0)
    }

    /// What the person, or the folder, said about it.
    #[must_use]
    pub fn said(&self) -> crate::book::Said {
        crate::book::Said {
            account: self.account_id,
            account_name: self.account_name.clone(),
            category: self.category_id,
        }
    }

    /// As it is answered.
    #[must_use]
    pub fn document(self) -> Document {
        let mut needs: Vec<Needed> = self.needs.iter().filter_map(|need| Needed::parse(need)).collect();
        needs.sort_unstable();
        Document {
            id: self.id,
            name: self.name,
            size: self.size,
            origin: self.origin,
            state: self.state,
            shop: self.receipt.as_ref().map(|receipt| crate::book::shop_name(&receipt.0)),
            receipt: self.receipt.map(|receipt| receipt.0),
            account_id: self.account_id,
            category_id: self.category_id,
            needs,
            reason: self.reason.map(|reason| reason.0),
            entry_id: self.entry_id,
            received_at: self.received_at,
            booked_at: self.booked_at,
        }
    }
}

/// The document a file already is, if this person has handed it over before.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn by_sha256(pool: &PgPool, owner_id: Uuid, sha256: &[u8]) -> Result<Option<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM documents WHERE owner_id = $1 AND sha256 = $2"
    )))
    .bind(owner_id)
    .bind(sha256)
    .fetch_optional(pool)
    .await
    .context("looking for the same file")
}

/// The document a receipt already is, by the receipt's own code.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn by_code(pool: &PgPool, owner_id: Uuid, code: &str) -> Result<Option<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM documents WHERE owner_id = $1 AND code = $2"
    )))
    .bind(owner_id)
    .bind(code)
    .fetch_optional(pool)
    .await
    .context("looking for the same receipt")
}

/// One document of this person's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn document(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT {COLUMNS} FROM documents WHERE owner_id = $1 AND id = $2")))
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a document")
}

/// A waiting document's file.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn content(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Vec<u8>>> {
    let found: Option<Option<Vec<u8>>> = sqlx::query_scalar("SELECT content FROM documents WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a document's file")?;
    Ok(found.flatten())
}

/// Which documents to list.
#[derive(Debug, Clone, Copy, Default)]
pub struct Filter {
    /// Only these.
    pub state: Option<State>,
    /// How many.
    pub limit: i64,
    /// How many to skip.
    pub offset: i64,
}

/// A person's documents, the newest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn documents(pool: &PgPool, owner_id: Uuid, filter: Filter) -> Result<Vec<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM documents
          WHERE owner_id = $1 AND ($2::document_state IS NULL OR state = $2)
          ORDER BY received_at DESC, id DESC
          LIMIT $3 OFFSET $4"
    )))
    .bind(owner_id)
    .bind(filter.state)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
    .context("listing documents")
}

/// A document arriving.
#[derive(Debug, Clone)]
pub struct Arriving<'a> {
    /// Whose.
    pub owner_id: Uuid,
    /// Its name.
    pub name: &'a str,
    /// Its bytes.
    pub content: &'a [u8],
    /// Their SHA-256.
    pub sha256: &'a [u8],
    /// How it arrived.
    pub origin: Origin,
}

/// Records a document that was read, waiting until it is booked; `None` when
/// the same file or receipt arrived at the same moment and is that one.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn arrive(pool: &PgPool, arriving: &Arriving<'_>, receipt: &Receipt, said: &crate::book::Said) -> Result<Option<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO documents (owner_id, name, size, sha256, content, origin, state, receipt, code, account_id, category_id, account_name)
         VALUES ($1, $2, $3, $4, $5, $6, 'waiting', $7, $8, $9, $10, $11)
         ON CONFLICT DO NOTHING
         RETURNING {COLUMNS}"
    )))
    .bind(arriving.owner_id)
    .bind(arriving.name)
    .bind(size(arriving.content)?)
    .bind(arriving.sha256)
    .bind(arriving.content)
    .bind(arriving.origin)
    .bind(Json(receipt))
    .bind(receipt.code.as_deref())
    .bind(said.account)
    .bind(said.category)
    .bind(said.account_name.as_deref())
    .fetch_optional(pool)
    .await
    .context("recording a document")
}

/// Records a document that cannot be read into the books, and why - without
/// its file, which there is nothing to do with.
///
/// A file refused before is refused again in place: what a newer build reads
/// is read as new, and what it still cannot is said once.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn refuse(pool: &PgPool, arriving: &Arriving<'_>, reason: &Reason) -> Result<Stored> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO documents (owner_id, name, size, sha256, origin, state, reason)
         VALUES ($1, $2, $3, $4, $5, 'refused', $6)
         ON CONFLICT (owner_id, sha256) DO UPDATE SET name = EXCLUDED.name, reason = EXCLUDED.reason, received_at = now()
           WHERE documents.state = 'refused'
         RETURNING {COLUMNS}"
    )))
    .bind(arriving.owner_id)
    .bind(arriving.name)
    .bind(size(arriving.content)?)
    .bind(arriving.sha256)
    .bind(arriving.origin)
    .bind(Json(reason))
    .fetch_optional(pool)
    .await
    .context("recording a refused document")?
    .map_or_else(
        || {
            // The file is a document in another state already - it arrived
            // read, at the same moment.
            Err(anyhow::anyhow!("the file was recorded as readable while it was being refused"))
        },
        Ok,
    )
}

/// Forgets a refused document, so its file is read afresh.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn forget_refused(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM documents WHERE owner_id = $1 AND id = $2 AND state = 'refused'")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("forgetting a refused document")?;
    Ok(())
}

/// Records that a document is in the books: its entry, and no file - the
/// entry keeps it now.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn booked(pool: &PgPool, owner_id: Uuid, id: Uuid, entry_id: Uuid) -> Result<Option<Stored>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE documents SET state = 'booked', entry_id = $3, booked_at = now(), content = NULL, needs = '{{}}', reason = NULL
          WHERE owner_id = $1 AND id = $2
          RETURNING {COLUMNS}"
    )))
    .bind(owner_id)
    .bind(id)
    .bind(entry_id)
    .fetch_optional(pool)
    .await
    .context("recording a document as booked")
}

/// Records what a document still waits for, and what has been said about it
/// so far.
///
/// A document booked in the meantime - a second attempt racing this one - is
/// left booked.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn waits(pool: &PgPool, owner_id: Uuid, id: Uuid, said: &crate::book::Said, needs: &[Needed], reason: Option<&Reason>) -> Result<Option<Stored>> {
    let needs: Vec<&str> = needs.iter().map(|need| need.as_str()).collect();
    let updated: Option<Stored> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE documents SET account_id = $3, category_id = $4, account_name = $5, needs = $6, reason = $7
          WHERE owner_id = $1 AND id = $2 AND state = 'waiting'
          RETURNING {COLUMNS}"
    )))
    .bind(owner_id)
    .bind(id)
    .bind(said.account)
    .bind(said.category)
    .bind(said.account_name.as_deref())
    .bind(&needs)
    .bind(reason.map(Json))
    .fetch_optional(pool)
    .await
    .context("recording what a document waits for")?;
    match updated {
        Some(stored) => Ok(Some(stored)),
        None => document(pool, owner_id, id).await,
    }
}

/// Dismisses a document: it leaves the list, and its file with it. An entry it
/// became stays in the books.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn dismiss(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM documents WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("dismissing a document")?;
    Ok(deleted.rows_affected() > 0)
}

/// A file's size as the database says it.
fn size(content: &[u8]) -> Result<i64> {
    i64::try_from(content.len()).context("a file larger than the database can say")
}
