//! Who, where and which labels: the counterparties, places and tags an entry
//! names.
//!
//! Each belongs to one person. That is held by the schema - every reference to
//! one names the owner as well as the row - so nothing here checks ownership
//! after the fact; a write naming another person's row fails at the foreign key.

use anyhow::{Context, Result};
use austeris_common::Actor;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{Change, CounterpartyChoice};
use crate::model::{Counterparty, CounterpartyKind, Place, Tag};

/// The columns a counterparty is read with.
const COUNTERPARTY: &str = "id, name, key, kind, default_category_id, tax_id";

/// Every counterparty a person has, by name.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn counterparties(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Counterparty>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COUNTERPARTY} FROM counterparties WHERE owner_id = $1 ORDER BY lower(name), id"
    )))
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing counterparties")
}

/// One counterparty, if it is this person's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn counterparty(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Counterparty>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COUNTERPARTY} FROM counterparties WHERE owner_id = $1 AND id = $2"
    )))
    .bind(owner_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading a counterparty")
}

/// The counterparty a typed word means: `casarica`, `casa-rica` and `Casa Rica`
/// are all "Casa Rica".
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn counterparty_by_name(pool: &PgPool, owner_id: Uuid, typed: &str) -> Result<Option<Counterparty>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COUNTERPARTY} FROM counterparties WHERE owner_id = $1 AND key = counterparty_key($2)"
    )))
    .bind(owner_id)
    .bind(typed)
    .fetch_optional(pool)
    .await
    .context("reading a counterparty by name")
}

/// The counterparty a receipt names: by its tax number when it carries one the
/// person has, otherwise by the key of its name.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn counterparty_as_named(pool: &PgPool, owner_id: Uuid, name: &str, tax_id: Option<&str>) -> Result<Option<Counterparty>> {
    // Ordered so the tax number wins: a shop typed by hand as "Casa Rica" and
    // printed as "CASA RICA S.A." is one shop once either has the number.
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COUNTERPARTY} FROM counterparties
          WHERE owner_id = $1 AND (tax_id = $3 OR key = counterparty_key($2))
          ORDER BY (tax_id IS NOT DISTINCT FROM $3) DESC
          LIMIT 1"
    )))
    .bind(owner_id)
    .bind(name)
    .bind(tax_id)
    .fetch_optional(pool)
    .await
    .context("finding a counterparty by tax number or name")
}

/// Creates a counterparty.
///
/// # Errors
///
/// Returns an error when the insert fails, including when the person has one
/// that is typed the same way or carries the same tax number, or names a
/// category that is not theirs.
pub async fn create_counterparty(
    pool: &PgPool,
    owner_id: Uuid,
    name: &str,
    kind: Option<CounterpartyKind>,
    default_category_id: Option<Uuid>,
    tax_id: Option<&str>,
) -> Result<Counterparty> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO counterparties (owner_id, name, kind, default_category_id, tax_id) VALUES ($1, $2, $3, $4, $5)
         RETURNING {COUNTERPARTY}"
    )))
    .bind(owner_id)
    .bind(name)
    .bind(kind)
    .bind(default_category_id)
    .bind(tax_id)
    .fetch_one(pool)
    .await
    .context("creating a counterparty")
}

/// What changes about a counterparty.
#[derive(Debug, Clone, Default)]
pub struct CounterpartyChanges {
    /// What to call it.
    pub name: Option<String>,
    /// What it is.
    pub kind: Change<CounterpartyKind>,
    /// What money spent there is usually for.
    pub default_category_id: Change<Uuid>,
    /// Its tax number.
    pub tax_id: Change<String>,
}

/// Changes a counterparty, returning it as it now is.
///
/// # Errors
///
/// Returns an error when the update fails, including when the new name types
/// the same as another of the person's counterparties.
pub async fn change_counterparty(pool: &PgPool, owner_id: Uuid, id: Uuid, changes: &CounterpartyChanges) -> Result<Option<Counterparty>> {
    // One statement: each field is set from its new value when one was given,
    // and kept otherwise. The flags say which, because `NULL` is also a value
    // a field can be changed to.
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE counterparties SET
             name = COALESCE($3, name),
             kind = CASE WHEN $4 THEN $5 ELSE kind END,
             default_category_id = CASE WHEN $6 THEN $7 ELSE default_category_id END,
             tax_id = CASE WHEN $8 THEN $9 ELSE tax_id END,
             updated_at = now()
          WHERE owner_id = $1 AND id = $2
          RETURNING {COUNTERPARTY}"
    )))
    .bind(owner_id)
    .bind(id)
    .bind(changes.name.as_deref())
    .bind(changes.kind.changes())
    .bind(changes.kind.value())
    .bind(changes.default_category_id.changes())
    .bind(changes.default_category_id.value())
    .bind(changes.tax_id.changes())
    .bind(changes.tax_id.clone().value())
    .fetch_optional(pool)
    .await
    .context("changing a counterparty")
}

/// Deletes a counterparty no entry names.
///
/// # Errors
///
/// Returns an error when the delete fails, including when entries still name
/// it - they would lose who they were with.
pub async fn delete_counterparty(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM counterparties WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("deleting a counterparty")?;
    Ok(deleted.rows_affected() > 0)
}

/// The counterparty an entry being written names, created when it is new.
pub(super) async fn choose_counterparty(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, choice: &CounterpartyChoice) -> Result<Uuid> {
    match choice {
        // Not looked up here: the foreign key, which names the owner, refuses
        // one that is not this person's.
        CounterpartyChoice::Existing(id) => Ok(*id),
        CounterpartyChoice::New { name, usual_category, tax_id } => {
            // Created, or found when one typed the same way - or carrying the
            // same tax number - appeared since the caller looked: two lines
            // naming a new shop at once are one shop.
            sqlx::query(
                "INSERT INTO counterparties (owner_id, name, default_category_id, tax_id) VALUES ($1, $2, $3, $4)
                 ON CONFLICT DO NOTHING",
            )
            .bind(owner_id)
            .bind(name)
            .bind(usual_category)
            .bind(tax_id.as_deref())
            .execute(&mut **transaction)
            .await
            .context("creating a counterparty")?;
            // The tax number first, as when looking one up: the row it names is
            // the shop, whatever this receipt calls it.
            let id: Uuid = sqlx::query_scalar(
                "SELECT id FROM counterparties
                  WHERE owner_id = $1 AND (tax_id = $3 OR key = counterparty_key($2))
                  ORDER BY (tax_id IS NOT DISTINCT FROM $3) DESC
                  LIMIT 1",
            )
            .bind(owner_id)
            .bind(name)
            .bind(tax_id.as_deref())
            .fetch_one(&mut **transaction)
            .await
            .context("finding the counterparty just named")?;
            // What it lacks, it takes; what it has, it keeps. A tax number
            // another of the person's counterparties already carries stays
            // there - the unique index would refuse it here, and which of the
            // two is the shop is the person's to say.
            sqlx::query(
                "UPDATE counterparties c SET
                     default_category_id = COALESCE(c.default_category_id, $3),
                     tax_id = CASE WHEN c.tax_id IS NULL
                                    AND NOT EXISTS (SELECT 1 FROM counterparties o WHERE o.owner_id = $1 AND o.tax_id = $4)
                                   THEN $4 ELSE c.tax_id END,
                     updated_at = now()
                  WHERE c.owner_id = $1 AND c.id = $2
                    AND ((c.default_category_id IS NULL AND $3::uuid IS NOT NULL) OR (c.tax_id IS NULL AND $4::text IS NOT NULL))",
            )
            .bind(owner_id)
            .bind(id)
            .bind(usual_category)
            .bind(tax_id.as_deref())
            .execute(&mut **transaction)
            .await
            .context("completing the counterparty just named")?;
            Ok(id)
        }
    }
}

/// Every place a person has recorded something at, by country and city.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn places(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Place>> {
    sqlx::query_as(
        "SELECT id, country::text AS country, city FROM places WHERE owner_id = $1
          ORDER BY country, city NULLS FIRST, id",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing places")
}

/// A place as a writer states it: a country, and a city when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlace {
    /// ISO 3166-1 alpha-2, upper case.
    pub country: String,
    /// The city, trimmed; `None` for the country as a whole.
    pub city: Option<String>,
}

/// Renames a place, returning it as it now is.
///
/// # Errors
///
/// Returns an error when the update fails, including when the person already
/// has a place by the new name.
pub async fn change_place(pool: &PgPool, owner_id: Uuid, id: Uuid, place: &NewPlace) -> Result<Option<Place>> {
    sqlx::query_as(
        "UPDATE places SET country = $3, city = $4, updated_at = now() WHERE owner_id = $1 AND id = $2
         RETURNING id, country::text AS country, city",
    )
    .bind(owner_id)
    .bind(id)
    .bind(&place.country)
    .bind(place.city.as_deref())
    .fetch_optional(pool)
    .await
    .context("renaming a place")
}

/// Letters with their accents, and what each is without them: the folding a
/// city's name is compared through. Listed rather than left to an extension,
/// which a stock PostgreSQL image does not load.
const ACCENTED: &str = "áàâäãåéèêëíìîïóòôöõúùûüñçý";
/// What each letter of [`ACCENTED`] is without its accent, in the same order.
const PLAIN: &str = "aaaaaaeeeeiiiiooooouuuuncy";

/// The place an entry being written names, created the first time.
///
/// A city is the same city however its accents were typed: a receipt printed
/// by a till that has none says `ASUNCION`, and it happened in the Asunción the
/// person already has. Found that way first; created under the name given only
/// when there is none.
pub(super) async fn find_or_create_place(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, place: &NewPlace) -> Result<Uuid> {
    let folded: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM places
          WHERE owner_id = $1 AND country = $2
            AND translate(lower(COALESCE(city, '')), $4, $5) = translate(lower(COALESCE($3, '')), $4, $5)
          ORDER BY (lower(COALESCE(city, '')) = lower(COALESCE($3, ''))) DESC, created_at
          LIMIT 1",
    )
    .bind(owner_id)
    .bind(&place.country)
    .bind(place.city.as_deref())
    .bind(ACCENTED)
    .bind(PLAIN)
    .fetch_optional(&mut **transaction)
    .await
    .context("looking for the place")?;
    if let Some(id) = folded {
        return Ok(id);
    }

    sqlx::query("INSERT INTO places (owner_id, country, city) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(owner_id)
        .bind(&place.country)
        .bind(place.city.as_deref())
        .execute(&mut **transaction)
        .await
        .context("recording a place")?;
    sqlx::query_scalar("SELECT id FROM places WHERE owner_id = $1 AND country = $2 AND lower(COALESCE(city, '')) = lower(COALESCE($3, ''))")
        .bind(owner_id)
        .bind(&place.country)
        .bind(place.city.as_deref())
        .fetch_one(&mut **transaction)
        .await
        .context("finding the place just named")
}

/// Every tag a person has, by name.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn tags(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Tag>> {
    sqlx::query_as("SELECT id, name FROM tags WHERE owner_id = $1 ORDER BY lower(name), id")
        .bind(owner_id)
        .fetch_all(pool)
        .await
        .context("listing tags")
}

/// Renames a tag, returning it as it now is.
///
/// # Errors
///
/// Returns an error when the update fails, including when the person already
/// has a tag by the new name.
pub async fn rename_tag(pool: &PgPool, owner_id: Uuid, id: Uuid, name: &str) -> Result<Option<Tag>> {
    sqlx::query_as("UPDATE tags SET name = $3, updated_at = now() WHERE owner_id = $1 AND id = $2 RETURNING id, name")
        .bind(owner_id)
        .bind(id)
        .bind(name)
        .fetch_optional(pool)
        .await
        .context("renaming a tag")
}

/// Deletes a tag, taking it off every entry that carried it.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn delete_tag(pool: &PgPool, owner_id: Uuid, by: &Actor, id: Uuid) -> Result<bool> {
    // As `by`'s: taking the tag off every entry carrying it is a change to
    // each of them, and their histories say who made it.
    let mut transaction = by.begin(pool).await?;
    let deleted = sqlx::query("DELETE FROM tags WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(&mut *transaction)
        .await
        .context("deleting a tag")?;
    transaction.commit().await.context("deleting a tag")?;
    Ok(deleted.rows_affected() > 0)
}

/// Puts tags on an entry being written, creating the ones used for the first
/// time. Names are matched in any case; a new one keeps the case it was
/// written in.
pub(super) async fn set_tags(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, entry_id: Uuid, names: &[String]) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    sqlx::query("INSERT INTO tags (owner_id, name) SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING")
        .bind(owner_id)
        .bind(names)
        .execute(&mut **transaction)
        .await
        .context("recording tags")?;
    sqlx::query(
        "INSERT INTO entry_tags (owner_id, entry_id, tag_id)
         SELECT $1, $2, t.id FROM tags t
          WHERE t.owner_id = $1 AND lower(t.name) IN (SELECT lower(unnest($3::text[])))
         ON CONFLICT DO NOTHING",
    )
    .bind(owner_id)
    .bind(entry_id)
    .bind(names)
    .execute(&mut **transaction)
    .await
    .context("tagging the entry")?;
    Ok(())
}
