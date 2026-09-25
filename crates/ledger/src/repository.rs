//! Reading and writing this service's schema.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::model::{Account, AccountKind, Balance, Category, Entry, ExchangeRate, Flow, Line, LineSide, Purpose, RateInForce};

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
        "SELECT id, parent_id, flow, name, purpose FROM categories WHERE owner_id = $1
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
        sqlx::query_as("SELECT id, parent_id, flow, name, purpose FROM categories WHERE owner_id = $1 AND lower(name) = $2")
            .bind(owner_id)
            .bind(&segments[0])
            .fetch_all(pool)
            .await
            .context("reading a category by name")?
    } else {
        sqlx::query_as(
            "WITH RECURSIVE walked AS (
                 SELECT c.id, c.parent_id, c.flow, c.name, c.purpose, 1 AS depth
                   FROM categories c
                  WHERE c.owner_id = $1 AND c.parent_id IS NULL AND lower(c.name) = ($2::text[])[1]
                 UNION ALL
                 SELECT c.id, c.parent_id, c.flow, c.name, c.purpose, w.depth + 1
                   FROM categories c
                   JOIN walked w ON c.parent_id = w.id
                  WHERE c.owner_id = $1 AND lower(c.name) = ($2::text[])[w.depth + 1]
             )
             SELECT id, parent_id, flow, name, purpose FROM walked WHERE depth = array_length($2::text[], 1)",
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
         RETURNING id, parent_id, flow, name, purpose",
    )
    .bind(owner_id)
    .bind(parent_id)
    .bind(flow)
    .bind(name)
    .fetch_one(pool)
    .await
    .context("creating a category")
}

/// The category the ledger files something under, created on first use.
///
/// Found by purpose, so a person who renamed "Exchange fees" to "Bank cuts" or
/// moved it under "Finance" still has their fees land in it. On first use it is
/// created at the root under its default name - or, when the person already has
/// a root expense category by that name, that one is adopted rather than a
/// second made beside it.
///
/// # Errors
///
/// Returns an error when the person has a root category by the default name
/// that is money coming in, which cannot hold fees, or when a write fails.
pub async fn purpose_category(pool: &PgPool, owner_id: Uuid, purpose: Purpose) -> Result<Category> {
    let by_purpose = || async {
        sqlx::query_as::<_, Category>("SELECT id, parent_id, flow, name, purpose FROM categories WHERE owner_id = $1 AND purpose = $2")
            .bind(owner_id)
            .bind(purpose)
            .fetch_optional(pool)
            .await
            .context("finding the ledger's own category")
    };

    if let Some(found) = by_purpose().await? {
        return Ok(found);
    }

    // `ON CONFLICT DO NOTHING` without a target: the insert can collide on the
    // name (the person made one by hand) or on the purpose (a concurrent
    // exchange got here first), and both are settled below rather than raised.
    let created: Option<Category> = sqlx::query_as(
        "INSERT INTO categories (owner_id, parent_id, flow, name, purpose) VALUES ($1, NULL, $2, $3, $4)
         ON CONFLICT DO NOTHING
         RETURNING id, parent_id, flow, name, purpose",
    )
    .bind(owner_id)
    .bind(purpose.flow())
    .bind(purpose.default_name())
    .bind(purpose)
    .fetch_optional(pool)
    .await
    .context("creating the ledger's own category")?;
    if let Some(created) = created {
        return Ok(created);
    }
    if let Some(found) = by_purpose().await? {
        return Ok(found);
    }

    let adopted: Option<Category> = sqlx::query_as(
        "UPDATE categories SET purpose = $4, updated_at = now()
          WHERE owner_id = $1 AND parent_id IS NULL AND lower(name) = lower($3) AND flow = $2 AND purpose IS NULL
          RETURNING id, parent_id, flow, name, purpose",
    )
    .bind(owner_id)
    .bind(purpose.flow())
    .bind(purpose.default_name())
    .bind(purpose)
    .fetch_optional(pool)
    .await
    .context("adopting a category for the ledger's use")?;

    adopted.ok_or_else(|| {
        anyhow::anyhow!(
            "a root category called `{}` is money coming in; rename it so the ledger can keep its own",
            purpose.default_name()
        )
    })
}

/// What a line is attributed to, as the code writing it states it.
///
/// An enum rather than two optional ids: a line naming both an account and a
/// category, or a conversion that names an account, cannot be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Money moving in or out of this account.
    Account(Uuid),
    /// Money earned from or spent on this category.
    Category(Uuid),
    /// Money changing currency inside the entry.
    Conversion,
}

impl Side {
    /// The side's kind, as it is stored.
    #[must_use]
    pub fn kind(self) -> LineSide {
        match self {
            Self::Account(_) => LineSide::Account,
            Self::Category(_) => LineSide::Category,
            Self::Conversion => LineSide::Conversion,
        }
    }

    /// The account, when this side is one.
    #[must_use]
    pub fn account(self) -> Option<Uuid> {
        match self {
            Self::Account(id) => Some(id),
            _ => None,
        }
    }

    /// The category, when this side is one.
    #[must_use]
    pub fn category(self) -> Option<Uuid> {
        match self {
            Self::Category(id) => Some(id),
            _ => None,
        }
    }
}

/// One side of an entry about to be written.
#[derive(Debug, Clone)]
pub struct NewLine {
    /// What this line is attributed to.
    pub side: Side,
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
            "INSERT INTO entry_lines (entry_id, side, account_id, category_id, amount, currency, note)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(entry_id)
        .bind(line.side.kind())
        .bind(line.side.account())
        .bind(line.side.category())
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
    let accounts: Vec<Uuid> = lines.iter().filter_map(|line| line.side.account()).collect();
    let categories: Vec<Uuid> = lines.iter().filter_map(|line| line.side.category()).collect();

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
type LineRow = (Uuid, Uuid, LineSide, Option<Uuid>, Option<Uuid>, Decimal, String, String);

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
        "SELECT entry_id, id, side, account_id, category_id, amount, currency, note
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
            side: row.2,
            account_id: row.3,
            category_id: row.4,
            amount: row.5,
            currency: row.6,
            note: row.7,
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
            rate_used: None,
        })
        .collect())
}

/// Records what one currency was worth in another on a day, replacing whatever
/// was recorded for that day before.
///
/// For a rate a person states: they are correcting the day, and what they typed
/// is what they mean. A source's rates go through [`record_observed_rates`],
/// which never replaces anything.
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

/// A rate a source published for a day.
#[derive(Debug, Clone)]
pub struct ObservedRate {
    /// The currency being priced.
    pub base: String,
    /// The currency it is priced in.
    pub quote: String,
    /// The day the source set it for.
    pub on_date: NaiveDate,
    /// How many of `quote` one `base` buys.
    pub rate: Decimal,
}

/// Records the rates a source published, keeping any already recorded.
///
/// Never replaces: a rate that valued a past entry must not change when a
/// source revises its history, and a rate a person typed for a day outranks
/// whatever a source says about it. Returns how many were new.
///
/// # Errors
///
/// Returns an error when the write fails.
pub async fn record_observed_rates(pool: &PgPool, source: &str, rates: &[ObservedRate]) -> Result<u64> {
    let mut recorded = 0;
    for rate in rates {
        recorded += sqlx::query(
            "INSERT INTO exchange_rates (base_currency, quote_currency, on_date, rate, source) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (base_currency, quote_currency, on_date) DO NOTHING",
        )
        .bind(&rate.base)
        .bind(&rate.quote)
        .bind(rate.on_date)
        .bind(rate.rate)
        .bind(source)
        .execute(pool)
        .await
        .context("recording a source's exchange rate")?
        .rows_affected();
    }
    Ok(recorded)
}

/// The rate in force on a day, with its age.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn rate_in_force(pool: &PgPool, base: &str, quote: &str, on_date: NaiveDate) -> Result<Option<RateInForce>> {
    Ok(rate_at(pool, base, quote, on_date).await?.map(|rate| RateInForce::new(rate, on_date)))
}

/// The rate to use for a day: the newest one not after it.
///
/// "As of", like a price: Sunday's conversion uses Friday's rate, because that
/// is the last thing anyone observed. A rate from *after* the day would be
/// answering a different question.
///
/// The pair may be answered three ways - as recorded, recorded the other way
/// round and inverted, or through a third currency both have a rate with. The
/// freshest answer wins, and on the same day a stated rate beats a derived one.
/// That is what lets a guarani be priced in roubles when one central bank
/// publishes guaranies per dollar and another roubles per dollar, and neither
/// publishes the pair.
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

    // The newest rate of every pair touching either currency, in one query -
    // everything any of the three answers could be built from.
    let known: Vec<ExchangeRate> = sqlx::query_as(
        "SELECT DISTINCT ON (base_currency, quote_currency) base_currency, quote_currency, on_date, rate, source
           FROM exchange_rates
          WHERE (base_currency IN ($1, $2) OR quote_currency IN ($1, $2)) AND on_date <= $3
          ORDER BY base_currency, quote_currency, on_date DESC",
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .fetch_all(pool)
    .await
    .context("reading exchange rates")?;

    Ok(best_rate(&known, base, quote))
}

/// Picks the answer for `base` into `quote` out of the rates known.
///
/// Separate from the query so the choice can be tested without a database.
fn best_rate(known: &[ExchangeRate], base: &str, quote: &str) -> Option<ExchangeRate> {
    // The pair itself first, as stated or inverted; then through a third
    // currency. Ranked by date, and by that order on the same date.
    let mut candidates: Vec<(NaiveDate, u8, ExchangeRate)> = Vec::new();
    if let Some(pair) = leg(known, base, quote) {
        candidates.push((pair.on_date, 2, pair));
    }

    // Every currency with a rate to `base` and a rate to `quote`, either way
    // round. USD first among equals: it is what both central banks this ledger
    // reads publish against, so it is the pivot a person would check by hand.
    let mut pivots: Vec<&str> = known
        .iter()
        .flat_map(|rate| [rate.base_currency.as_str(), rate.quote_currency.as_str()])
        .filter(|currency| *currency != base && *currency != quote)
        .collect();
    pivots.sort_unstable_by_key(|currency| (*currency != "USD", *currency));
    pivots.dedup();
    for pivot in pivots.into_iter().rev() {
        if let (Some(first), Some(second)) = (leg(known, base, pivot), leg(known, pivot, quote))
            && let Some(rate) = first.rate.checked_mul(second.rate)
        {
            let on_date = first.on_date.min(second.on_date);
            candidates.push((
                on_date,
                1,
                ExchangeRate {
                    base_currency: base.to_owned(),
                    quote_currency: quote.to_owned(),
                    on_date,
                    rate,
                    source: format!("{} via {pivot}", joined_sources(&first.source, &second.source)),
                },
            ));
        }
    }

    // Newest wins; on the same day the pair itself beats a cross, and among
    // crosses USD does - it was pushed last, and `max_by_key` keeps the last
    // of equal keys.
    candidates.into_iter().max_by_key(|(date, rank, _)| (*date, *rank)).map(|(_, _, rate)| rate)
}

/// The rate from `from` into `to` among those known, as stated or inverted.
fn leg(known: &[ExchangeRate], from: &str, to: &str) -> Option<ExchangeRate> {
    let stated = known.iter().find(|rate| rate.base_currency == from && rate.quote_currency == to);
    let inverted = known.iter().find(|rate| rate.base_currency == to && rate.quote_currency == from);
    match (stated, inverted) {
        (Some(stated), Some(inverted)) if inverted.on_date > stated.on_date => Some(invert(inverted)),
        (Some(stated), _) => Some(stated.clone()),
        (None, Some(inverted)) => Some(invert(inverted)),
        (None, None) => None,
    }
}

/// A rate read the other way round.
///
/// Recording PYG->USD does not oblige anyone to also record USD->PYG: they are
/// the same fact said twice, and two rows that must agree are two rows that can
/// disagree. `Decimal::ONE / rate` keeps twenty-odd significant digits, which is
/// more than any rate is quoted to.
fn invert(rate: &ExchangeRate) -> ExchangeRate {
    ExchangeRate {
        base_currency: rate.quote_currency.clone(),
        quote_currency: rate.base_currency.clone(),
        on_date: rate.on_date,
        rate: Decimal::ONE / rate.rate,
        source: rate.source.clone(),
    }
}

/// Two sources named once each: `bcp` and `bcp` is `bcp`, not `bcp+bcp`.
fn joined_sources(first: &str, second: &str) -> String {
    if first == second { first.to_owned() } else { format!("{first}+{second}") }
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    use super::best_rate;
    use crate::model::ExchangeRate;

    fn rate(base: &str, quote: &str, on: u32, value: &str, source: &str) -> ExchangeRate {
        ExchangeRate {
            base_currency: base.to_owned(),
            quote_currency: quote.to_owned(),
            on_date: NaiveDate::from_ymd_opt(2026, 9, on).expect("a date"),
            rate: Decimal::from_str(value).expect("a decimal"),
            source: source.to_owned(),
        }
    }

    #[test]
    fn a_pair_nobody_publishes_is_priced_through_the_currency_both_publish_against() {
        // The Paraguayan central bank prices dollars in guaranies, the Russian
        // one dollars in roubles. Neither prices guaranies in roubles.
        let known = [rate("USD", "PYG", 24, "5900", "bcp"), rate("USD", "RUB", 25, "85", "cbr")];
        let cross = best_rate(&known, "PYG", "RUB").expect("a cross rate");
        // 1 PYG = 1/5900 USD = 85/5900 RUB.
        assert_eq!(cross.rate.round_dp(12), Decimal::from_str("0.014406779661").expect("a decimal"));
        // As old as its older leg, and saying where it came from.
        assert_eq!(cross.on_date, NaiveDate::from_ymd_opt(2026, 9, 24).expect("a date"));
        assert_eq!(cross.source, "bcp+cbr via USD");
    }

    #[test]
    fn a_stated_rate_beats_a_derived_one_of_the_same_day_and_loses_to_a_fresher_one() {
        let known = [
            rate("PYG", "RUB", 20, "0.0140", "manual"),
            rate("USD", "PYG", 24, "5900", "bcp"),
            rate("USD", "RUB", 24, "85", "cbr"),
        ];
        // The cross rate is four days fresher than the one a person typed.
        assert!(best_rate(&known, "PYG", "RUB").expect("a rate").source.contains("via USD"));

        let known = [
            rate("PYG", "RUB", 24, "0.0140", "manual"),
            rate("USD", "PYG", 24, "5900", "bcp"),
            rate("USD", "RUB", 24, "85", "cbr"),
        ];
        assert_eq!(best_rate(&known, "PYG", "RUB").expect("a rate").source, "manual");
    }

    #[test]
    fn a_rate_recorded_the_other_way_round_answers_inverted() {
        let known = [rate("USD", "PYG", 24, "5000", "bcp")];
        let inverted = best_rate(&known, "PYG", "USD").expect("an inverted rate");
        assert_eq!(inverted.rate, Decimal::from_str("0.0002").expect("a decimal"));
        assert_eq!(inverted.base_currency, "PYG");
    }

    #[test]
    fn nothing_known_is_no_answer_rather_than_a_guess() {
        let known = [rate("USD", "PYG", 24, "5900", "bcp")];
        assert!(best_rate(&known, "EUR", "RUB").is_none());
    }
}
