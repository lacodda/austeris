//! What money was spent on and earned from, gathered by category, tag,
//! counterparty, place or country.
//!
//! One query shape for all five. Each sums the lines on categories - what the
//! money was for - and only the grouping changes, so "what did the holiday go
//! on" is the category grouping filtered to a tag, and "who got how much" the
//! counterparty grouping, with the same rules for what counts.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Flowed, Group, GroupBy};

/// A group's sums in one currency, as they come back: the group, its name, the
/// currency, spent and earned.
type TotalsRow = (Option<Uuid>, Option<String>, String, Decimal, Decimal);

/// Which lines to sum.
#[derive(Debug, Clone, Default)]
pub struct TotalsFilter {
    /// Only entries on or after this day.
    pub from: Option<NaiveDate>,
    /// Only entries on or before this day.
    pub to: Option<NaiveDate>,
    /// Only entries carrying this tag, by name in any case.
    pub tag: Option<String>,
    /// Only entries with this counterparty.
    pub counterparty_id: Option<Uuid>,
    /// Only entries moving this account.
    pub account_id: Option<Uuid>,
}

/// The part every grouping shares: which lines, and what is summed of them.
///
/// Held entries count: the money was spent the day it was spent, whenever the
/// bank gets round to posting it. A day is the day it happened, not the day it
/// posted, for the same reason.
macro_rules! totals {
    ($group:literal, $join:literal) => {
        concat!(
            "SELECT ",
            $group,
            ", l.currency,
                    COALESCE(SUM(l.amount) FILTER (WHERE c.flow = 'expense'), 0) AS spent,
                    COALESCE(-SUM(l.amount) FILTER (WHERE c.flow = 'income'), 0) AS earned
               FROM entry_lines l
               JOIN entries e ON e.id = l.entry_id
               JOIN categories c ON c.id = l.category_id ",
            $join,
            " WHERE e.owner_id = $1 AND l.side = 'category'
                AND ($2::date IS NULL OR e.occurred_on >= $2)
                AND ($3::date IS NULL OR e.occurred_on <= $3)
                AND ($4::text IS NULL OR EXISTS (SELECT 1 FROM entry_tags ft JOIN tags tt ON tt.id = ft.tag_id
                                                  WHERE ft.entry_id = e.id AND lower(tt.name) = lower($4)))
                AND ($5::uuid IS NULL OR e.counterparty_id = $5)
                AND ($6::uuid IS NULL OR EXISTS (SELECT 1 FROM entry_lines fa WHERE fa.entry_id = e.id AND fa.account_id = $6))
              GROUP BY 1, 2, l.currency
              ORDER BY 2 NULLS LAST, 1, l.currency"
        )
    };
}

/// The lines on categories, summed per group and currency.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn totals(pool: &PgPool, owner_id: Uuid, by: GroupBy, filter: &TotalsFilter) -> Result<Vec<Group>> {
    let query = match by {
        GroupBy::Category => totals!("c.id, c.name", ""),
        GroupBy::Tag => totals!(
            "t.id, t.name",
            "LEFT JOIN entry_tags et ON et.entry_id = e.id LEFT JOIN tags t ON t.id = et.tag_id"
        ),
        GroupBy::Counterparty => totals!("cp.id, cp.name", "LEFT JOIN counterparties cp ON cp.id = e.counterparty_id"),
        GroupBy::Place => totals!(
            "p.id, CASE WHEN p.city IS NULL THEN p.country::text ELSE p.city || ', ' || p.country END",
            "LEFT JOIN places p ON p.id = e.place_id"
        ),
        GroupBy::Country => totals!("NULL::uuid, p.country::text", "LEFT JOIN places p ON p.id = e.place_id"),
    };

    let rows: Vec<TotalsRow> = sqlx::query_as(query)
        .bind(owner_id)
        .bind(filter.from)
        .bind(filter.to)
        .bind(filter.tag.as_deref())
        .bind(filter.counterparty_id)
        .bind(filter.account_id)
        .fetch_all(pool)
        .await
        .context("adding up what money went on")?;

    // The rows come ordered by group, so each group's currencies are
    // consecutive.
    let mut groups: Vec<Group> = Vec::new();
    for (id, name, currency, spent, earned) in rows {
        let flowed = Flowed { currency, spent, earned };
        match groups.last_mut() {
            Some(last) if last.id == id && last.name == name => last.amounts.push(flowed),
            _ => groups.push(Group {
                id,
                name,
                amounts: vec![flowed],
            }),
        }
    }
    Ok(groups)
}
