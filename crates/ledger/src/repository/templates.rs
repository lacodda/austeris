//! Templates: buttons that record the same payment again.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::Change;
use crate::model::{CounterpartyRef, Flow, Place, Purpose, Template};
use crate::paid::{self, NotOne};

/// What a template is read with: its own columns, its category's flow, who and
/// where, and its tags.
const TEMPLATE_SELECT: &str = "SELECT t.id, t.name, t.account_id, t.category_id, c.flow, t.amount, t.currency, t.description, t.pending,
        cp.id AS counterparty_id, cp.name AS counterparty_name,
        p.id AS place_id, p.country::text AS country, p.city,
        ARRAY(SELECT tg.name FROM template_tags tt JOIN tags tg ON tg.id = tt.tag_id
               WHERE tt.template_id = t.id ORDER BY lower(tg.name)) AS tags
   FROM templates t
   JOIN categories c ON c.id = t.category_id
   LEFT JOIN counterparties cp ON cp.id = t.counterparty_id
   LEFT JOIN places p ON p.id = t.place_id";

/// A template as it comes back.
#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    account_id: Uuid,
    category_id: Uuid,
    flow: Flow,
    amount: Option<Decimal>,
    currency: String,
    description: String,
    pending: bool,
    counterparty_id: Option<Uuid>,
    counterparty_name: Option<String>,
    place_id: Option<Uuid>,
    country: Option<String>,
    city: Option<String>,
    tags: Vec<String>,
}

impl From<Row> for Template {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            name: row.name,
            account_id: row.account_id,
            category_id: row.category_id,
            flow: row.flow,
            amount: row.amount.map(|amount| amount.normalize()),
            currency: row.currency,
            description: row.description,
            counterparty: row.counterparty_id.zip(row.counterparty_name).map(|(id, name)| CounterpartyRef { id, name }),
            place: row.place_id.zip(row.country).map(|(id, country)| Place { id, country, city: row.city }),
            tags: row.tags,
            pending: row.pending,
        }
    }
}

/// Every template a person has, in the order their buttons sit.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn templates(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Template>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{TEMPLATE_SELECT} WHERE t.owner_id = $1 ORDER BY t.position, lower(t.name), t.id"
    )))
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing templates")?;
    Ok(rows.into_iter().map(Template::from).collect())
}

/// One template, if it is this person's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn template(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Template>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{TEMPLATE_SELECT} WHERE t.owner_id = $1 AND t.id = $2")))
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading a template")?;
    Ok(row.map(Template::from))
}

/// Why an entry was not made a template.
#[derive(Debug, thiserror::Error)]
pub enum NoTemplate {
    /// There is no such entry of this person's.
    #[error("no such entry")]
    NoEntry,
    /// The entry is not one payment.
    #[error("{0}; a template is one payment from one account on one category")]
    NotOnePayment(#[from] NotOne),
    /// A module keeps the entry: a trade's money, which only that module
    /// records again.
    #[error("this entry is kept by {0}, which records another")]
    Kept(String),
}

/// Makes a button out of an entry, as the last of the person's buttons.
///
/// The entry has to be one payment: one account, one category, and the
/// conversion between them when it was charged in another currency. Its
/// amount is kept unless `ask_amount` says to ask for it each time, and it is
/// held when the entry went through a hold.
///
/// # Errors
///
/// Returns [`NoTemplate`] when there is no such entry or it is not one
/// payment, and an error when the write fails - including when the person
/// already has a template by that name.
pub async fn template_from_entry(pool: &PgPool, owner_id: Uuid, entry_id: Uuid, name: &str, ask_amount: bool) -> Result<Template> {
    let entry = super::entry(pool, owner_id, entry_id).await?.ok_or(NoTemplate::NoEntry)?;
    if entry.kept {
        return Err(NoTemplate::Kept(entry.source.unwrap_or_default()).into());
    }
    let fee_category = super::category_for(pool, owner_id, Purpose::ExchangeFees).await?.map(|category| category.id);
    let paid = paid::of(&entry, fee_category).map_err(NoTemplate::from)?;
    let from = TemplateFrom {
        entry_id,
        name: name.to_owned(),
        account_id: paid.account_id,
        category_id: paid.category_id,
        amount: (!ask_amount).then_some(paid.amount),
        currency: paid.currency,
        pending: was_held(entry.occurred_on, entry.cleared_on),
    };
    Ok(create_template(pool, owner_id, &from).await?.ok_or(NoTemplate::NoEntry)?)
}

/// A template made from an entry: what its payment is, and what to call it.
#[derive(Debug, Clone)]
struct TemplateFrom {
    /// The entry it is made from; who, where, the description and the tags
    /// are taken from it.
    pub entry_id: Uuid,
    /// What the button says.
    pub name: String,
    /// The account the payment moves.
    pub account_id: Uuid,
    /// What it is for.
    pub category_id: Uuid,
    /// How much, or `None` to ask each time.
    pub amount: Option<Decimal>,
    /// What the amount is in.
    pub currency: String,
    /// Each entry is held rather than posted.
    pub pending: bool,
}

/// Writes a template made from an entry.
///
/// Who, where, the description and the tags are copied from the entry in the
/// same statement that reads them, so the template is the entry as it is now.
async fn create_template(pool: &PgPool, owner_id: Uuid, from: &TemplateFrom) -> Result<Option<Template>> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO templates (owner_id, name, account_id, category_id, amount, currency, description, counterparty_id, place_id, pending, position)
         SELECT e.owner_id, $3, $4, $5, $6, $7, e.description, e.counterparty_id, e.place_id, $8,
                COALESCE((SELECT max(position) + 1 FROM templates WHERE owner_id = $1), 0)
           FROM entries e WHERE e.owner_id = $1 AND e.id = $2
         RETURNING id",
    )
    .bind(owner_id)
    .bind(from.entry_id)
    .bind(&from.name)
    .bind(from.account_id)
    .bind(from.category_id)
    .bind(from.amount)
    .bind(&from.currency)
    .bind(from.pending)
    .fetch_optional(&mut *transaction)
    .await
    .context("making a template")?;
    let Some(id) = id else {
        return Ok(None);
    };

    sqlx::query(
        "INSERT INTO template_tags (owner_id, template_id, tag_id)
         SELECT owner_id, $2, tag_id FROM entry_tags WHERE owner_id = $1 AND entry_id = $3",
    )
    .bind(owner_id)
    .bind(id)
    .bind(from.entry_id)
    .execute(&mut *transaction)
    .await
    .context("copying the entry's tags onto the template")?;

    transaction.commit().await.context("making a template")?;
    template(pool, owner_id, id).await
}

/// What changes about a template.
#[derive(Debug, Clone, Default)]
pub struct TemplateChanges {
    /// What the button says.
    pub name: Option<String>,
    /// How much; cleared, it asks each time.
    pub amount: Change<Decimal>,
}

/// Renames a template or changes its amount, returning it as it now is.
///
/// # Errors
///
/// Returns an error when the update fails, including when the new name is
/// another template's.
pub async fn change_template(pool: &PgPool, owner_id: Uuid, id: Uuid, changes: &TemplateChanges) -> Result<Option<Template>> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "UPDATE templates
            SET name = COALESCE($3, name),
                amount = CASE WHEN $4 THEN $5 ELSE amount END,
                updated_at = now()
          WHERE owner_id = $1 AND id = $2
          RETURNING id",
    )
    .bind(owner_id)
    .bind(id)
    .bind(changes.name.as_deref())
    .bind(changes.amount.changes())
    .bind(changes.amount.value())
    .fetch_optional(pool)
    .await
    .context("changing a template")?;
    match found {
        Some(id) => template(pool, owner_id, id).await,
        None => Ok(None),
    }
}

/// Puts a person's templates in the order given.
///
/// Returns `false` when the ids are not exactly the person's templates, each
/// once - an order that leaves one out has no place for it.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn order_templates(pool: &PgPool, owner_id: Uuid, ids: &[Uuid]) -> Result<bool> {
    let mut transaction = pool.begin().await.context("opening a transaction")?;
    let mut theirs: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM templates WHERE owner_id = $1 FOR UPDATE")
        .bind(owner_id)
        .fetch_all(&mut *transaction)
        .await
        .context("reading the templates to order")?;
    let mut given = ids.to_vec();
    theirs.sort_unstable();
    given.sort_unstable();
    if theirs != given {
        return Ok(false);
    }

    sqlx::query(
        "UPDATE templates t SET position = o.position - 1, updated_at = now()
           FROM unnest($2::uuid[]) WITH ORDINALITY AS o(id, position)
          WHERE t.owner_id = $1 AND t.id = o.id",
    )
    .bind(owner_id)
    .bind(ids)
    .execute(&mut *transaction)
    .await
    .context("ordering templates")?;
    transaction.commit().await.context("ordering templates")?;
    Ok(true)
}

/// Deletes a template. The entries it recorded stay: they are the person's
/// money, and the button was only a way of writing them.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn delete_template(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM templates WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("deleting a template")?;
    Ok(deleted.rows_affected() > 0)
}

/// Whether an entry went through a hold: held now, or posted on a later day
/// than it was made.
#[must_use]
pub fn was_held(occurred_on: NaiveDate, cleared_on: Option<NaiveDate>) -> bool {
    cleared_on.is_none_or(|posted| posted > occurred_on)
}
