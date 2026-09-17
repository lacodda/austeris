//! Reading and writing this service's schema.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::model::{Account, AccountKind, Balance, Category, Entry, ExchangeRate, Flow, Line};

/// Every account a person has, open ones first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn accounts(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Account>> {
    sqlx::query_as(
        "SELECT id, kind, name, currency, opening_balance, closed_at
         FROM accounts WHERE owner_id = $1
         ORDER BY closed_at NULLS FIRST, kind, lower(name)",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing accounts")
}

/// One account, if it is this person's.
///
/// The owner is part of the lookup rather than checked afterwards: a query that
/// can return another person's row is one that eventually does.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn account(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Account>> {
    sqlx::query_as("SELECT id, kind, name, currency, opening_balance, closed_at FROM accounts WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading an account")
}

/// The account a name refers to, matched as a person would write it.
///
/// Used by the one-line parser's caller: `45000 food from cash` names an
/// account by what the person calls it, not by a uuid.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn account_by_name(pool: &PgPool, owner_id: Uuid, name: &str) -> Result<Option<Account>> {
    sqlx::query_as(
        "SELECT id, kind, name, currency, opening_balance, closed_at
         FROM accounts WHERE owner_id = $1 AND lower(name) = lower($2)",
    )
    .bind(owner_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .context("reading an account by name")
}

/// Creates an account.
///
/// # Errors
///
/// Returns an error when the insert fails, including when the person already
/// has one by that name.
pub async fn create_account(pool: &PgPool, owner_id: Uuid, kind: AccountKind, name: &str, currency: &str, opening_balance: Decimal) -> Result<Account> {
    sqlx::query_as(
        "INSERT INTO accounts (owner_id, kind, name, currency, opening_balance) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, kind, name, currency, opening_balance, closed_at",
    )
    .bind(owner_id)
    .bind(kind)
    .bind(name)
    .bind(currency)
    .bind(opening_balance)
    .fetch_one(pool)
    .await
    .context("creating an account")
}

/// Closes an account, or reopens it.
///
/// # Errors
///
/// Returns an error when the update fails.
pub async fn set_account_closed(pool: &PgPool, owner_id: Uuid, id: Uuid, closed: bool) -> Result<Option<Account>> {
    sqlx::query_as(
        "UPDATE accounts SET closed_at = CASE WHEN $3 THEN now() ELSE NULL END, updated_at = now()
         WHERE owner_id = $1 AND id = $2
         RETURNING id, kind, name, currency, opening_balance, closed_at",
    )
    .bind(owner_id)
    .bind(id)
    .bind(closed)
    .fetch_optional(pool)
    .await
    .context("closing an account")
}

/// Every category a person has, parents before their children.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn categories(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Category>> {
    sqlx::query_as(
        "SELECT id, parent_id, flow, name FROM categories WHERE owner_id = $1
         ORDER BY flow, parent_id NULLS FIRST, lower(name)",
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .context("listing categories")
}

/// The category a name refers to.
///
/// A leaf's own name is enough (`food`), and so is a path (`living/food`): a
/// person typing a line says the shortest thing that identifies it. When two
/// leaves share a name under different parents, the path is what tells them
/// apart, and a bare name matching both returns neither - answering with one of
/// them silently files the money in the wrong place.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn category_by_name(pool: &PgPool, owner_id: Uuid, name: &str) -> Result<CategoryMatch> {
    // A path is resolved by walking it: `living/food` is `food` whose parent is
    // `living`. Done in one query with a recursive CTE, because doing it in
    // Rust would be a round trip per segment.
    let segments: Vec<String> = name.split('/').map(|part| part.trim().to_lowercase()).filter(|part| !part.is_empty()).collect();
    if segments.is_empty() {
        return Ok(CategoryMatch::None);
    }

    let matches: Vec<Category> = if segments.len() == 1 {
        sqlx::query_as("SELECT id, parent_id, flow, name FROM categories WHERE owner_id = $1 AND lower(name) = $2")
            .bind(owner_id)
            .bind(&segments[0])
            .fetch_all(pool)
            .await
            .context("reading a category by name")?
    } else {
        sqlx::query_as(
            "WITH RECURSIVE walked AS (
                 SELECT c.id, c.parent_id, c.flow, c.name, 1 AS depth
                   FROM categories c
                  WHERE c.owner_id = $1 AND c.parent_id IS NULL AND lower(c.name) = ($2::text[])[1]
                 UNION ALL
                 SELECT c.id, c.parent_id, c.flow, c.name, w.depth + 1
                   FROM categories c
                   JOIN walked w ON c.parent_id = w.id
                  WHERE c.owner_id = $1 AND lower(c.name) = ($2::text[])[w.depth + 1]
             )
             SELECT id, parent_id, flow, name FROM walked WHERE depth = array_length($2::text[], 1)",
        )
        .bind(owner_id)
        .bind(&segments)
        .fetch_all(pool)
        .await
        .context("reading a category by path")?
    };

    Ok(match <[Category; 1]>::try_from(matches) {
        Ok([only]) => CategoryMatch::One(Box::new(only)),
        Err(rest) if rest.is_empty() => CategoryMatch::None,
        // Several leaves share this name. Picking one would file the money
        // somewhere the person did not say; the names are handed back so the
        // caller can ask which.
        Err(rest) => CategoryMatch::Several(rest.into_iter().map(|category| category.name).collect()),
    })
}

/// What looking a category up by name found.
#[derive(Debug)]
pub enum CategoryMatch {
    /// Exactly one.
    One(Box<Category>),
    /// None by that name.
    None,
    /// Several, by their names. Ambiguous on purpose rather than resolved.
    Several(Vec<String>),
}

/// Creates a category.
///
/// # Errors
///
/// Returns an error when the insert fails, including when a sibling already has
/// that name.
pub async fn create_category(pool: &PgPool, owner_id: Uuid, parent_id: Option<Uuid>, flow: Flow, name: &str) -> Result<Category> {
    sqlx::query_as(
        "INSERT INTO categories (owner_id, parent_id, flow, name) VALUES ($1, $2, $3, $4)
         RETURNING id, parent_id, flow, name",
    )
    .bind(owner_id)
    .bind(parent_id)
    .bind(flow)
    .bind(name)
    .fetch_one(pool)
    .await
    .context("creating a category")
}

/// One side of an entry about to be written.
#[derive(Debug, Clone)]
pub struct NewLine {
    /// The account this side moves, when it is an account.
    pub account_id: Option<Uuid>,
    /// The category this side is attributed to, when it is a category.
    pub category_id: Option<Uuid>,
    /// The signed amount.
    pub amount: Decimal,
    /// What this line is in.
    pub currency: String,
    /// What this side was for.
    pub note: String,
}

/// An entry about to be written.
#[derive(Debug, Clone)]
pub struct NewEntry {
    /// The day the money moved.
    pub occurred_on: NaiveDate,
    /// What a person would call it.
    pub description: String,
    /// A module's own name for this movement, so a retry does not book twice.
    pub idempotency_key: Option<String>,
    /// Which module posted it.
    pub source: Option<String>,
    /// The sides of the movement.
    pub lines: Vec<NewLine>,
}

/// What posting an entry did.
#[derive(Debug)]
pub struct Posted {
    /// The entry, as it is now stored.
    pub entry: Entry,
    /// Whether this call created it, or the idempotency key had been used and
    /// this is the entry from that first call.
    pub created: bool,
}

/// Records a movement.
///
/// Header and lines go in one transaction, and the balance rule is checked at
/// its commit - the constraint trigger is deferred, so the first line of a
/// two-line entry does not fail on its own. An entry that does not balance is
/// refused here; nothing repairs it, because a repaired entry is one nobody
/// typed.
///
/// # Errors
///
/// Returns an error when the entry does not balance, when it references
/// something that is not this person's, or when the write fails.
pub async fn post_entry(pool: &PgPool, owner_id: Uuid, new: &NewEntry) -> Result<Posted> {
    // An idempotency key that has already been used is answered from the
    // existing entry without opening a transaction that would only roll back.
    if let Some(key) = &new.idempotency_key
        && let Some(existing) = entry_by_key(pool, owner_id, key).await?
    {
        return Ok(Posted {
            entry: existing,
            created: false,
        });
    }

    let mut transaction = pool.begin().await.context("opening a transaction")?;

    // Every account and category named has to be this person's. Checked here
    // rather than trusted from the caller: the gateway says who is calling, and
    // a module posting with a stale id must not reach into someone else's books.
    // The foreign keys alone would not catch it - they point at the table, not
    // at the owner.
    ensure_sides_belong_to(&mut transaction, owner_id, &new.lines).await?;

    let entry_id: Uuid = sqlx::query_scalar(
        "INSERT INTO entries (owner_id, occurred_on, description, idempotency_key, source)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(owner_id)
    .bind(new.occurred_on)
    .bind(&new.description)
    .bind(new.idempotency_key.as_deref())
    .bind(new.source.as_deref())
    .fetch_one(&mut *transaction)
    .await
    .context("creating an entry")?;

    for line in &new.lines {
        sqlx::query(
            "INSERT INTO entry_lines (entry_id, account_id, category_id, amount, currency, note)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(entry_id)
        .bind(line.account_id)
        .bind(line.category_id)
        .bind(line.amount)
        .bind(&line.currency)
        .bind(&line.note)
        .execute(&mut *transaction)
        .await
        .context("adding a line to an entry")?;
    }

    // This is where an unbalanced entry fails: the trigger is deferred, so the
    // rule is applied to the whole entry at once.
    transaction.commit().await.context("recording the entry")?;

    let entry = entry(pool, owner_id, entry_id).await?.context("the entry vanished after being written")?;
    Ok(Posted { entry, created: true })
}

/// Refuses lines pointing at something that is not this person's.
async fn ensure_sides_belong_to(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, lines: &[NewLine]) -> Result<()> {
    let accounts: Vec<Uuid> = lines.iter().filter_map(|line| line.account_id).collect();
    let categories: Vec<Uuid> = lines.iter().filter_map(|line| line.category_id).collect();

    // One query per kind rather than one per line: an entry can have many
    // lines, and a round trip each would make a split receipt slow for no
    // reason.
    let accounts = dedup(&accounts);
    let found: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE owner_id = $1 AND id = ANY($2)")
        .bind(owner_id)
        .bind(&accounts)
        .fetch_one(&mut **transaction)
        .await
        .context("checking the accounts an entry names")?;
    anyhow::ensure!(
        usize::try_from(found).unwrap_or(0) == accounts.len(),
        "an entry names an account that does not exist"
    );

    let categories = dedup(&categories);
    let found: i64 = sqlx::query_scalar("SELECT count(*) FROM categories WHERE owner_id = $1 AND id = ANY($2)")
        .bind(owner_id)
        .bind(&categories)
        .fetch_one(&mut **transaction)
        .await
        .context("checking the categories an entry names")?;
    anyhow::ensure!(
        usize::try_from(found).unwrap_or(0) == categories.len(),
        "an entry names a category that does not exist"
    );

    Ok(())
}

/// The distinct ids in a list.
fn dedup(ids: &[Uuid]) -> Vec<Uuid> {
    let mut out = ids.to_vec();
    out.sort_unstable();
    out.dedup();
    out
}

/// One entry with its lines, if it is this person's.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn entry(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Entry>> {
    let Some(header) = sqlx::query_as::<_, (Uuid, NaiveDate, String, Option<String>)>(
        "SELECT id, occurred_on, description, source FROM entries WHERE owner_id = $1 AND id = $2",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("reading an entry")?
    else {
        return Ok(None);
    };

    Ok(Some(Entry {
        id: header.0,
        occurred_on: header.1,
        description: header.2,
        source: header.3,
        lines: lines_of(pool, &[header.0]).await?.remove(&header.0).unwrap_or_default(),
    }))
}

/// The entry an idempotency key already produced, if any.
async fn entry_by_key(pool: &PgPool, owner_id: Uuid, key: &str) -> Result<Option<Entry>> {
    let Some(id): Option<Uuid> = sqlx::query_scalar("SELECT id FROM entries WHERE owner_id = $1 AND idempotency_key = $2")
        .bind(owner_id)
        .bind(key)
        .fetch_optional(pool)
        .await
        .context("looking up an idempotency key")?
    else {
        return Ok(None);
    };

    entry(pool, owner_id, id).await
}

/// Which entries to read.
#[derive(Debug, Clone, Default)]
pub struct EntryFilter {
    /// Only entries on or after this day.
    pub from: Option<NaiveDate>,
    /// Only entries on or before this day.
    pub to: Option<NaiveDate>,
    /// Only entries with a line touching this account.
    pub account_id: Option<Uuid>,
    /// Only entries with a line attributed to this category.
    pub category_id: Option<Uuid>,
    /// How many to return.
    pub limit: i64,
    /// How many to skip.
    pub offset: i64,
}

/// Entries matching a filter, newest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn entries(pool: &PgPool, owner_id: Uuid, filter: &EntryFilter) -> Result<Vec<Entry>> {
    // The headers are selected first and their lines fetched in one further
    // query: a join would repeat each header once per line and the pagination
    // would then count lines rather than entries, which is not what a page of
    // entries means.
    let headers: Vec<(Uuid, NaiveDate, String, Option<String>)> = sqlx::query_as(
        "SELECT e.id, e.occurred_on, e.description, e.source
           FROM entries e
          WHERE e.owner_id = $1
            AND ($2::date IS NULL OR e.occurred_on >= $2)
            AND ($3::date IS NULL OR e.occurred_on <= $3)
            AND ($4::uuid IS NULL OR EXISTS (SELECT 1 FROM entry_lines l WHERE l.entry_id = e.id AND l.account_id = $4))
            AND ($5::uuid IS NULL OR EXISTS (SELECT 1 FROM entry_lines l WHERE l.entry_id = e.id AND l.category_id = $5))
          ORDER BY e.occurred_on DESC, e.created_at DESC, e.id DESC
          LIMIT $6 OFFSET $7",
    )
    .bind(owner_id)
    .bind(filter.from)
    .bind(filter.to)
    .bind(filter.account_id)
    .bind(filter.category_id)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
    .context("listing entries")?;

    let ids: Vec<Uuid> = headers.iter().map(|header| header.0).collect();
    let mut lines = lines_of(pool, &ids).await?;

    Ok(headers
        .into_iter()
        .map(|header| Entry {
            id: header.0,
            occurred_on: header.1,
            description: header.2,
            source: header.3,
            lines: lines.remove(&header.0).unwrap_or_default(),
        })
        .collect())
}

/// A row of `entry_lines` as it comes back: the entry it belongs to, then the
/// line's own columns in the order they are selected.
type LineRow = (Uuid, Uuid, Option<Uuid>, Option<Uuid>, Decimal, String, String);

/// Every line of the entries named, grouped by the entry it belongs to.
///
/// One query for the whole page rather than one per entry: a page of fifty
/// entries is one round trip, not fifty-one.
async fn lines_of(pool: &PgPool, entry_ids: &[Uuid]) -> Result<HashMap<Uuid, Vec<Line>>> {
    if entry_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Ordered by amount so the money leaving comes before the money arriving,
    // which is how a person reads an entry; `id` breaks the tie, because a
    // split with two equal parts must not shuffle between reads.
    let rows: Vec<LineRow> = sqlx::query_as(
        "SELECT entry_id, id, account_id, category_id, amount, currency, note
           FROM entry_lines WHERE entry_id = ANY($1)
          ORDER BY entry_id, amount, id",
    )
    .bind(entry_ids)
    .fetch_all(pool)
    .await
    .context("reading the lines of an entry")?;

    let mut grouped: HashMap<Uuid, Vec<Line>> = HashMap::new();
    for row in rows {
        grouped.entry(row.0).or_default().push(Line {
            id: row.1,
            account_id: row.2,
            category_id: row.3,
            amount: row.4,
            currency: row.5,
            note: row.6,
        });
    }
    Ok(grouped)
}

/// Deletes an entry and its lines.
///
/// # Errors
///
/// Returns an error when the delete fails.
pub async fn delete_entry(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<bool> {
    let deleted = sqlx::query("DELETE FROM entries WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(pool)
        .await
        .context("deleting an entry")?;
    Ok(deleted.rows_affected() > 0)
}

/// What every account held at the end of a day.
///
/// Opening balance plus every line up to and including that day. Computed, never
/// stored: a balance kept beside the lines is a second copy of the same truth,
/// and the day they disagree there is no way to tell which is right.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn balances(pool: &PgPool, owner_id: Uuid, as_of: NaiveDate) -> Result<Vec<Balance>> {
    let rows: Vec<(Uuid, String, String, Decimal)> = sqlx::query_as(
        "SELECT a.id, a.name, a.currency,
                a.opening_balance + COALESCE(SUM(l.amount) FILTER (WHERE e.occurred_on <= $2), 0)
           FROM accounts a
           LEFT JOIN entry_lines l ON l.account_id = a.id
           LEFT JOIN entries e ON e.id = l.entry_id
          WHERE a.owner_id = $1
          GROUP BY a.id, a.name, a.currency, a.opening_balance
          ORDER BY a.kind, lower(a.name)",
    )
    .bind(owner_id)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .context("reading balances")?;

    Ok(rows
        .into_iter()
        .map(|row| Balance {
            account_id: row.0,
            name: row.1,
            currency: row.2,
            amount: row.3,
            converted: None,
        })
        .collect())
}

/// Records what one currency was worth in another on a day.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn record_rate(pool: &PgPool, base: &str, quote: &str, on_date: NaiveDate, rate: Decimal, source: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO exchange_rates (base_currency, quote_currency, on_date, rate, source) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (base_currency, quote_currency, on_date) DO UPDATE SET rate = EXCLUDED.rate, source = EXCLUDED.source",
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .bind(rate)
    .bind(source)
    .execute(pool)
    .await
    .context("recording an exchange rate")?;
    Ok(())
}

/// The rate to use for a day: the newest one not after it.
///
/// "As of", like a price: Sunday's conversion uses Friday's rate, because that
/// is the last thing anyone observed. A rate from *after* the day would be
/// answering a different question.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn rate_at(pool: &PgPool, base: &str, quote: &str, on_date: NaiveDate) -> Result<Option<ExchangeRate>> {
    if base.eq_ignore_ascii_case(quote) {
        // A currency in itself is one, and storing that row for every currency
        // would be a table of tautologies to keep up to date.
        return Ok(Some(ExchangeRate {
            base_currency: base.to_owned(),
            quote_currency: quote.to_owned(),
            on_date,
            rate: Decimal::ONE,
            source: "identity".to_owned(),
        }));
    }

    let direct: Option<ExchangeRate> = sqlx::query_as(
        "SELECT base_currency, quote_currency, on_date, rate, source FROM exchange_rates
          WHERE base_currency = $1 AND quote_currency = $2 AND on_date <= $3
          ORDER BY on_date DESC LIMIT 1",
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .fetch_optional(pool)
    .await
    .context("reading an exchange rate")?;

    if direct.is_some() {
        return Ok(direct);
    }

    // The other way round, inverted. Recording PYG->USD should not oblige
    // anyone to also record USD->PYG: they are the same fact said twice, and
    // two rows that must agree are two rows that can disagree.
    let inverse: Option<ExchangeRate> = sqlx::query_as(
        "SELECT base_currency, quote_currency, on_date, rate, source FROM exchange_rates
          WHERE base_currency = $2 AND quote_currency = $1 AND on_date <= $3
          ORDER BY on_date DESC LIMIT 1",
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .fetch_optional(pool)
    .await
    .context("reading an inverted exchange rate")?;

    Ok(inverse.map(|found| ExchangeRate {
        base_currency: base.to_owned(),
        quote_currency: quote.to_owned(),
        on_date: found.on_date,
        // `Decimal::ONE / rate` at this scale keeps twenty-odd significant
        // digits, which is more than any rate is quoted to.
        rate: Decimal::ONE / found.rate,
        source: found.source,
    }))
}

/// Every rate recorded for a pair, newest first.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn rates(pool: &PgPool, base: &str, quote: &str, limit: i64) -> Result<Vec<ExchangeRate>> {
    sqlx::query_as(
        "SELECT base_currency, quote_currency, on_date, rate, source FROM exchange_rates
          WHERE base_currency = $1 AND quote_currency = $2
          ORDER BY on_date DESC LIMIT $3",
    )
    .bind(base)
    .bind(quote)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("listing exchange rates")
}
