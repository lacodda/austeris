//! Sorting the books out: entries changed in batches, and a counterparty
//! merged into another.
//!
//! Each batch is one transaction: a hundred entries are given their category
//! together, or none of them is, and every one of them has a revision in its
//! history saying who did it - the triggers write those as they do for a
//! single change (ADR 0013).

use anyhow::{Context, Result};
use austeris_common::Actor;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{counterparty, details::set_tags};
use crate::model::Counterparty;

/// The most entries one batch changes.
pub const BATCH_LIMIT: usize = 500;

/// What a batch does to every entry in it.
#[derive(Debug, Clone)]
pub enum Batch {
    /// Files the money under this category: every line not yet sorted, and
    /// every line on a category of the person's own. Lines on a category the
    /// ledger or a module keeps - the fees an exchange took - stay where they
    /// are; an entry with nothing to file - a transfer between accounts - is
    /// left as it is.
    Categorize(Uuid),
    /// Moves the money from one account to another of the same currency: the
    /// entries were recorded against the wrong one.
    Move {
        /// Where the entries say the money moved now.
        from: Uuid,
        /// Where it moved.
        to: Uuid,
    },
    /// Puts labels on and takes labels off. A label used for the first time is
    /// created.
    Tag {
        /// Labels to put on, by name.
        add: Vec<String>,
        /// Labels to take off, by name in any case.
        remove: Vec<String>,
    },
}

/// Why a batch was not done. Nothing in it was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchRefusal {
    /// Some of the entries are not the person's, or do not exist.
    Missing {
        /// How many.
        count: usize,
    },
    /// The category is not the person's.
    UnknownCategory,
    /// An account named is not the person's.
    UnknownAccount,
    /// Money is moved to the account it is on.
    SameAccount,
    /// The account money is moved to is closed.
    ClosedAccount {
        /// The account.
        name: String,
    },
    /// The two accounts are in different currencies: moving the money would
    /// need a rate nobody stated.
    OtherCurrency {
        /// What the money is in now.
        from: String,
        /// What the account it is moved to is in.
        to: String,
    },
    /// Some of the entries move money between the two accounts already: moved,
    /// they would move it from an account to itself.
    OntoItself {
        /// How many.
        count: usize,
    },
    /// Some of the entries are kept by a module - a trade's money - and their
    /// money changes there.
    Kept {
        /// How many.
        count: usize,
        /// Which module keeps them.
        module: String,
    },
}

/// What a batch changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batched {
    /// The entries it changed.
    pub changed: Vec<Uuid>,
    /// The entries it left as they were: nothing in them was what it changes.
    pub unchanged: Vec<Uuid>,
}

/// Does a batch to the person's entries named, all of it or none of it.
///
/// # Errors
///
/// Returns an error when a query fails; a batch that cannot be done as asked
/// is the inner [`BatchRefusal`].
pub async fn batch(pool: &PgPool, owner_id: Uuid, by: &Actor, ids: &[Uuid], what: &Batch) -> Result<Result<Batched, BatchRefusal>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();

    let mut transaction = by.begin(pool).await?;
    // Locked, in one order, so two batches over the same entries take turns
    // rather than deadlock.
    let found: Vec<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, CASE WHEN kept THEN source END FROM entries WHERE owner_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE")
            .bind(owner_id)
            .bind(&ids)
            .fetch_all(&mut *transaction)
            .await
            .context("finding the entries of a batch")?;
    if found.len() != ids.len() {
        return Ok(Err(BatchRefusal::Missing {
            count: ids.len() - found.len(),
        }));
    }

    let changed = match what {
        Batch::Categorize(category) => match categorize(&mut transaction, owner_id, &ids, *category).await? {
            Ok(changed) => changed,
            Err(refusal) => return Ok(Err(refusal)),
        },
        Batch::Move { from, to } => match move_between(&mut transaction, owner_id, &ids, &found, *from, *to).await? {
            Ok(changed) => changed,
            Err(refusal) => return Ok(Err(refusal)),
        },
        Batch::Tag { add, remove } => tag(&mut transaction, owner_id, &ids, add, remove).await?,
    };
    transaction.commit().await.context("changing a batch of entries")?;

    let unchanged = ids.iter().filter(|id| !changed.contains(id)).copied().collect();
    Ok(Ok(Batched { changed, unchanged }))
}

/// Files the money of the entries under a category.
async fn categorize(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, ids: &[Uuid], category: Uuid) -> Result<Result<Vec<Uuid>, BatchRefusal>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM categories WHERE owner_id = $1 AND id = $2)")
        .bind(owner_id)
        .bind(category)
        .fetch_one(&mut **transaction)
        .await
        .context("finding the category of a batch")?;
    if !exists {
        return Ok(Err(BatchRefusal::UnknownCategory));
    }
    let mut changed: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE entry_lines l SET side = 'category', category_id = $3
          WHERE l.entry_id = ANY($2)
            AND l.category_id IS DISTINCT FROM $3
            AND (l.side = 'unsorted'
                 OR (l.side = 'category' AND EXISTS (SELECT 1 FROM categories c
                                                      WHERE c.owner_id = $1 AND c.id = l.category_id
                                                        AND c.purpose IS NULL AND c.module IS NULL)))
          RETURNING l.entry_id",
    )
    .bind(owner_id)
    .bind(ids)
    .bind(category)
    .fetch_all(&mut **transaction)
    .await
    .context("filing a batch of entries under a category")?;
    changed.sort_unstable();
    changed.dedup();
    Ok(Ok(changed))
}

/// Moves the money of the entries from one account to another.
async fn move_between(
    transaction: &mut Transaction<'_, Postgres>,
    owner_id: Uuid,
    ids: &[Uuid],
    found: &[(Uuid, Option<String>)],
    from: Uuid,
    to: Uuid,
) -> Result<Result<Vec<Uuid>, BatchRefusal>> {
    if from == to {
        return Ok(Err(BatchRefusal::SameAccount));
    }
    let accounts: Vec<(Uuid, String, String, bool)> =
        sqlx::query_as("SELECT id, name, currency, closed_at IS NOT NULL FROM accounts WHERE owner_id = $1 AND id IN ($2, $3)")
            .bind(owner_id)
            .bind(from)
            .bind(to)
            .fetch_all(&mut **transaction)
            .await
            .context("finding the accounts of a batch")?;
    let (Some(source), Some(target)) = (
        accounts.iter().find(|account| account.0 == from),
        accounts.iter().find(|account| account.0 == to),
    ) else {
        return Ok(Err(BatchRefusal::UnknownAccount));
    };
    if target.3 {
        return Ok(Err(BatchRefusal::ClosedAccount { name: target.1.clone() }));
    }
    if source.2 != target.2 {
        return Ok(Err(BatchRefusal::OtherCurrency {
            from: source.2.clone(),
            to: target.2.clone(),
        }));
    }

    // Which of the entries the move reaches: those with money on `from`.
    let reached: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT entry_id, bool_or(account_id = $3) FROM entry_lines
          WHERE entry_id = ANY($1) AND account_id IN ($2, $3)
          GROUP BY entry_id
         HAVING bool_or(account_id = $2)",
    )
    .bind(ids)
    .bind(from)
    .bind(to)
    .fetch_all(&mut **transaction)
    .await
    .context("finding the money a batch moves")?;
    let onto_itself = reached.iter().filter(|(_, both)| *both).count();
    if onto_itself > 0 {
        return Ok(Err(BatchRefusal::OntoItself { count: onto_itself }));
    }
    let kept: Vec<&String> = found
        .iter()
        .filter(|(id, _)| reached.iter().any(|(reached, _)| reached == id))
        .filter_map(|(_, module)| module.as_ref())
        .collect();
    if let Some(module) = kept.first() {
        return Ok(Err(BatchRefusal::Kept {
            count: kept.len(),
            module: (*module).clone(),
        }));
    }

    let mut changed: Vec<Uuid> = sqlx::query_scalar("UPDATE entry_lines SET account_id = $3 WHERE entry_id = ANY($1) AND account_id = $2 RETURNING entry_id")
        .bind(ids)
        .bind(from)
        .bind(to)
        .fetch_all(&mut **transaction)
        .await
        .context("moving a batch of entries to another account")?;
    changed.sort_unstable();
    changed.dedup();
    Ok(Ok(changed))
}

/// Puts labels on the entries and takes labels off them.
async fn tag(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, ids: &[Uuid], add: &[String], remove: &[String]) -> Result<Vec<Uuid>> {
    let mut changed: Vec<Uuid> = Vec::new();
    if !remove.is_empty() {
        let taken: Vec<Uuid> = sqlx::query_scalar(
            "DELETE FROM entry_tags et USING tags t
              WHERE et.owner_id = $1 AND et.entry_id = ANY($2) AND t.id = et.tag_id
                AND lower(t.name) IN (SELECT lower(unnest($3::text[])))
             RETURNING et.entry_id",
        )
        .bind(owner_id)
        .bind(ids)
        .bind(remove)
        .fetch_all(&mut **transaction)
        .await
        .context("taking labels off a batch of entries")?;
        changed.extend(taken);
    }
    if !add.is_empty() {
        let before: Vec<(Uuid, i64)> = sqlx::query_as("SELECT entry_id, count(*) FROM entry_tags WHERE owner_id = $1 AND entry_id = ANY($2) GROUP BY entry_id")
            .bind(owner_id)
            .bind(ids)
            .fetch_all(&mut **transaction)
            .await
            .context("counting the labels of a batch of entries")?;
        for id in ids {
            set_tags(transaction, owner_id, *id, add).await?;
        }
        let after: Vec<(Uuid, i64)> = sqlx::query_as("SELECT entry_id, count(*) FROM entry_tags WHERE owner_id = $1 AND entry_id = ANY($2) GROUP BY entry_id")
            .bind(owner_id)
            .bind(ids)
            .fetch_all(&mut **transaction)
            .await
            .context("counting the labels of a batch of entries")?;
        let count = |rows: &[(Uuid, i64)], id: Uuid| rows.iter().find(|(entry, _)| *entry == id).map_or(0, |(_, count)| *count);
        changed.extend(ids.iter().copied().filter(|id| count(&after, *id) > count(&before, *id)));
    }
    changed.sort_unstable();
    changed.dedup();
    Ok(changed)
}

/// Why a counterparty was not merged into another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRefusal {
    /// One of the two is not the person's.
    Missing,
    /// A counterparty merged into itself.
    Itself,
    /// Both carry a tax number, and not the same one: two businesses, not one
    /// typed two ways.
    TaxIdsDiffer {
        /// The one merged away's.
        from: String,
        /// The one it is merged into's.
        into: String,
    },
}

/// What a merge did.
#[derive(Debug, Clone)]
pub struct Merged {
    /// The counterparty merged into, as it is now.
    pub into: Counterparty,
    /// How many entries it moved.
    pub entries: u64,
    /// How many buttons it moved.
    pub templates: u64,
}

/// Merges a counterparty into another, in one transaction: its entries and its
/// buttons name the other from now on, the other takes what it lacks of its
/// usual category, its kind and its tax number, the way it was typed - and any
/// way it was itself merged from - becomes a way of typing the other, and it
/// is deleted.
///
/// # Errors
///
/// Returns an error when a query fails; a merge that cannot be done is the
/// inner [`MergeRefusal`].
pub async fn merge_counterparty(pool: &PgPool, owner_id: Uuid, by: &Actor, from: Uuid, into: Uuid) -> Result<Result<Merged, MergeRefusal>> {
    if from == into {
        return Ok(Err(MergeRefusal::Itself));
    }
    let mut transaction = by.begin(pool).await?;
    let both: Vec<(Uuid, String, Option<String>)> =
        sqlx::query_as("SELECT id, key, tax_id FROM counterparties WHERE owner_id = $1 AND id IN ($2, $3) ORDER BY id FOR UPDATE")
            .bind(owner_id)
            .bind(from)
            .bind(into)
            .fetch_all(&mut *transaction)
            .await
            .context("finding the counterparties to merge")?;
    let (Some(gone), Some(kept)) = (both.iter().find(|row| row.0 == from), both.iter().find(|row| row.0 == into)) else {
        return Ok(Err(MergeRefusal::Missing));
    };
    if let (Some(gone_tax), Some(kept_tax)) = (&gone.2, &kept.2)
        && gone_tax != kept_tax
    {
        return Ok(Err(MergeRefusal::TaxIdsDiffer {
            from: gone_tax.clone(),
            into: kept_tax.clone(),
        }));
    }
    let gone_key = gone.1.clone();

    let entries = sqlx::query("UPDATE entries SET counterparty_id = $3, updated_at = now() WHERE owner_id = $1 AND counterparty_id = $2")
        .bind(owner_id)
        .bind(from)
        .bind(into)
        .execute(&mut *transaction)
        .await
        .context("moving the entries of a merged counterparty")?
        .rows_affected();
    let templates = sqlx::query("UPDATE templates SET counterparty_id = $3, updated_at = now() WHERE owner_id = $1 AND counterparty_id = $2")
        .bind(owner_id)
        .bind(from)
        .bind(into)
        .execute(&mut *transaction)
        .await
        .context("moving the buttons of a merged counterparty")?
        .rows_affected();

    // What it was merged from goes along, before it is deleted and takes its
    // aliases with it.
    sqlx::query("UPDATE counterparty_aliases SET counterparty_id = $3 WHERE owner_id = $1 AND counterparty_id = $2")
        .bind(owner_id)
        .bind(from)
        .bind(into)
        .execute(&mut *transaction)
        .await
        .context("moving what a merged counterparty was typed as")?;
    // Deleted first and then read from what it was: its tax number cannot be
    // on two of the person's counterparties at once, even for a moment.
    let (usual, kind, tax_id): (Option<Uuid>, Option<String>, Option<String>) =
        sqlx::query_as("DELETE FROM counterparties WHERE owner_id = $1 AND id = $2 RETURNING default_category_id, kind::text, tax_id")
            .bind(owner_id)
            .bind(from)
            .fetch_one(&mut *transaction)
            .await
            .context("deleting a merged counterparty")?;
    sqlx::query(
        "UPDATE counterparties SET
             default_category_id = COALESCE(default_category_id, $3),
             kind = COALESCE(kind, $4::counterparty_kind),
             tax_id = COALESCE(tax_id, $5),
             updated_at = now()
          WHERE owner_id = $1 AND id = $2",
    )
    .bind(owner_id)
    .bind(into)
    .bind(usual)
    .bind(kind)
    .bind(tax_id)
    .execute(&mut *transaction)
    .await
    .context("completing the counterparty merged into")?;
    if gone_key != kept.1 {
        sqlx::query("INSERT INTO counterparty_aliases (owner_id, key, counterparty_id) VALUES ($1, $2, $3) ON CONFLICT (owner_id, key) DO UPDATE SET counterparty_id = $3")
            .bind(owner_id)
            .bind(&gone_key)
            .bind(into)
            .execute(&mut *transaction)
            .await
            .context("keeping what a merged counterparty was typed as")?;
    }
    transaction.commit().await.context("merging a counterparty")?;

    let merged = counterparty(pool, owner_id, into).await?.context("the counterparty merged into vanished")?;
    Ok(Ok(Merged {
        into: merged,
        entries,
        templates,
    }))
}
