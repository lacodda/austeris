//! What happened to an entry: read from the revisions the database keeps.
//!
//! The database writes the history (see the migration that adds it); this
//! reads it back. A revision keeps the entry as it was before it, so the entry
//! after a revision is the one before the next - or the entry as it is now,
//! read through the same function the database snapshots with.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::model::{Attachment, CounterpartyRef, Entry, EntryField, EntryStatus, Line, LineSide, Place, Revision, RevisionAction, Tag, Who, WhoKind};

/// An entry as the database snapshots it: everything a person reads on it,
/// with the ids that say which account, category, tag or counterparty each
/// part was.
#[derive(Debug, Clone, Deserialize)]
pub struct Snapshot {
    /// The day the money moved.
    pub occurred_on: NaiveDate,
    /// Whether the bank had posted it.
    pub status: EntryStatus,
    /// The day it posted.
    pub cleared_on: Option<NaiveDate>,
    /// What it was called.
    pub description: String,
    /// Which module posted it.
    pub source: Option<String>,
    /// A module's own name for it, which a retry finds it by.
    pub idempotency_key: Option<String>,
    /// Kept by that module. Absent from a snapshot taken before modules kept
    /// entries, which none was.
    #[serde(default)]
    pub kept: bool,
    /// When it was first recorded.
    pub created_at: DateTime<Utc>,
    /// Who it was with, named as they were then.
    pub counterparty: Option<CounterpartyRef>,
    /// Where it happened.
    pub place: Option<Place>,
    /// Its labels, by id and by the name each had then.
    pub tags: Vec<Tag>,
    /// Its sides, with their ids.
    pub lines: Vec<Line>,
    /// The files that were on it.
    pub attachments: Vec<Attachment>,
}

impl Snapshot {
    /// The entry this was, as the rest of the API shows an entry.
    #[must_use]
    pub fn into_entry(self, id: Uuid) -> Entry {
        Entry {
            id,
            occurred_on: self.occurred_on,
            status: self.status,
            cleared_on: self.cleared_on,
            description: self.description,
            source: self.source,
            kept: self.kept,
            counterparty: self.counterparty,
            place: self.place,
            tags: self.tags.into_iter().map(|tag| tag.name).collect(),
            lines: self.lines,
            attachments: self.attachments,
        }
    }

    /// What it is called in a list: its description, or who it was with.
    #[must_use]
    pub fn title(&self) -> String {
        match (self.description.as_str(), &self.counterparty) {
            ("", Some(counterparty)) => counterparty.name.clone(),
            (description, _) => description.to_owned(),
        }
    }
}

/// What changed between two states of one entry, in the order an entry is
/// read.
///
/// By what each part *is* rather than how it reads: a tag renamed since, or a
/// counterparty, is the same tag and the same counterparty, and a hold posted
/// for the amount it was held for has the same lines whatever ids they were
/// written again under.
#[must_use]
pub fn changed(before: &Snapshot, after: &Snapshot) -> Vec<EntryField> {
    let mut out = Vec::new();
    if before.occurred_on != after.occurred_on {
        out.push(EntryField::OccurredOn);
    }
    if before.description != after.description {
        out.push(EntryField::Description);
    }
    if posting(before) != posting(after) {
        out.push(EntryField::Status);
    }
    if before.counterparty.as_ref().map(|c| c.id) != after.counterparty.as_ref().map(|c| c.id) {
        out.push(EntryField::Counterparty);
    }
    if before.place.as_ref().map(|p| p.id) != after.place.as_ref().map(|p| p.id) {
        out.push(EntryField::Place);
    }
    let tags = |snapshot: &Snapshot| snapshot.tags.iter().map(|tag| tag.id).collect::<BTreeSet<_>>();
    if tags(before) != tags(after) {
        out.push(EntryField::Tags);
    }
    if moved(before) != moved(after) {
        out.push(EntryField::Lines);
    }
    let files = |snapshot: &Snapshot| snapshot.attachments.iter().map(|file| file.id).collect::<BTreeSet<_>>();
    if files(before) != files(after) {
        out.push(EntryField::Attachments);
    }
    out
}

/// When it posted, as a person reads it: held, posted the day it happened, or
/// posted on a day of its own. An entry posted the day it happened and moved
/// to another day is still posted the day it happened - its status did not
/// change, its day did.
fn posting(snapshot: &Snapshot) -> Posting {
    match (snapshot.status, snapshot.cleared_on) {
        (EntryStatus::Pending, _) => Posting::Held,
        (EntryStatus::Cleared, Some(day)) if day != snapshot.occurred_on => Posting::On(day),
        (EntryStatus::Cleared, _) => Posting::TheDayItHappened,
    }
}

/// When an entry posted, as a person reads it.
#[derive(Debug, PartialEq, Eq)]
enum Posting {
    Held,
    TheDayItHappened,
    On(NaiveDate),
}

/// A line as the money it moves: which side - by rank, so lines sort - which
/// account or category, how much, in what, and why.
type Moved = (u8, Option<Uuid>, Option<Uuid>, Decimal, String, String);

/// An entry's lines as the money they move, in an order of their own.
fn moved(snapshot: &Snapshot) -> Vec<Moved> {
    let mut out: Vec<Moved> = snapshot
        .lines
        .iter()
        .map(|line| {
            let rank = match line.side {
                LineSide::Account => 0,
                LineSide::Category => 1,
                LineSide::Conversion => 2,
                LineSide::Holding => 3,
            };
            (
                rank,
                line.account_id,
                line.category_id,
                line.amount.normalize(),
                line.currency.clone(),
                line.note.clone(),
            )
        })
        .collect();
    out.sort();
    out
}

/// A revision as it is stored.
#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    action: RevisionAction,
    actor: WhoKind,
    actor_person: Option<Uuid>,
    actor_module: Option<String>,
    at: DateTime<Utc>,
    before: Option<Json<Snapshot>>,
}

impl Row {
    fn who(&self) -> Who {
        Who {
            kind: self.actor,
            person_id: self.actor_person,
            module: self.actor_module.clone(),
        }
    }
}

/// The entry as it is now, as the database snapshots it - when it is this
/// person's and not in the trash.
///
/// # Errors
///
/// Returns an error when the query fails or the snapshot does not read.
pub async fn snapshot(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Snapshot>> {
    let found: Option<Json<Snapshot>> = sqlx::query_scalar("SELECT entry_snapshot(id) FROM entries WHERE owner_id = $1 AND id = $2")
        .bind(owner_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("reading what an entry is now")?;
    Ok(found.map(|json| json.0))
}

/// Everything that happened to an entry, oldest first.
///
/// For an entry in the books and for one in the trash alike. `None` when the
/// person has no entry by that id and never had.
///
/// # Errors
///
/// Returns an error when a query fails or a stored snapshot does not read.
pub async fn history(pool: &PgPool, owner_id: Uuid, id: Uuid) -> Result<Option<Vec<Revision>>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, action, actor, actor_person, actor_module, at, before
           FROM entry_revisions WHERE owner_id = $1 AND entry_id = $2
          ORDER BY id",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_all(pool)
    .await
    .context("reading an entry's history")?;
    let now = snapshot(pool, owner_id, id).await?;
    if rows.is_empty() && now.is_none() {
        return Ok(None);
    }

    // The state after each revision is the state before the next one.
    let mut afters: Vec<Option<Snapshot>> = rows.iter().skip(1).map(|row| row.before.as_ref().map(|json| json.0.clone())).collect();
    afters.push(now);

    let mut revisions = Vec::with_capacity(rows.len());
    for (row, after) in rows.into_iter().zip(afters) {
        let after = if row.action == RevisionAction::Deleted { None } else { after };
        let before = row.before.as_ref().map(|json| json.0.clone());
        let fields = match (&row.action, &before, &after) {
            (RevisionAction::Changed, Some(before), Some(after)) => changed(before, after),
            _ => Vec::new(),
        };
        // A change whose every difference is in how something reads - a tag
        // renamed in the same transaction - changed nothing about the entry.
        if row.action == RevisionAction::Changed && fields.is_empty() {
            continue;
        }
        revisions.push(Revision {
            id: row.id,
            at: row.at,
            action: row.action,
            who: row.who(),
            changed: fields,
            before: before.map(|snapshot| snapshot.into_entry(id)),
            after: after.map(|snapshot| snapshot.into_entry(id)),
        });
    }
    Ok(Some(revisions))
}

/// Whether anything has happened to a person's entries beyond their being
/// recorded: a change, a deletion, a file kept with one.
///
/// # Errors
///
/// Returns an error when the query fails.
pub async fn anything_remembered(pool: &PgPool, owner_id: Uuid) -> Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM entry_revisions WHERE owner_id = $1 AND action <> 'created')
             OR EXISTS (SELECT 1 FROM attachments WHERE owner_id = $1)",
    )
    .bind(owner_id)
    .fetch_one(pool)
    .await
    .context("looking for what the books remember")
}

/// The revision an entry's history ends in, when it ends in its deletion and
/// the entry is not back: the entry in the trash, as it was deleted.
pub(super) struct Deletion {
    pub at: DateTime<Utc>,
    pub who: Who,
    pub before: Snapshot,
}

/// How an entry in the trash is found, for reading and for restoring alike:
/// its history's last word is its deletion, and it is not in the books.
///
/// An entry a module kept is not among them: the module took it back with
/// whatever it came from - a trade - and only it could bring it back. Its
/// history stays readable by id until the trash's time is up.
macro_rules! deletions_select {
    () => {
        "SELECT r.entry_id, r.at, r.actor, r.actor_person, r.actor_module, r.before
           FROM entry_revisions r
          WHERE r.owner_id = $1 AND r.action = 'deleted'
            AND NOT COALESCE((r.before->>'kept')::boolean, false)
            AND r.at > now() - make_interval(days => $2)
            AND r.id = (SELECT max(x.id) FROM entry_revisions x WHERE x.entry_id = r.entry_id)
            AND NOT EXISTS (SELECT 1 FROM entries e WHERE e.id = r.entry_id)"
    };
}
pub(super) use deletions_select;

/// A deletion as it is read.
#[derive(sqlx::FromRow)]
pub(super) struct DeletionRow {
    pub entry_id: Uuid,
    pub at: DateTime<Utc>,
    pub actor: WhoKind,
    pub actor_person: Option<Uuid>,
    pub actor_module: Option<String>,
    pub before: Json<Snapshot>,
}

impl DeletionRow {
    pub(super) fn into_deletion(self) -> (Uuid, Deletion) {
        (
            self.entry_id,
            Deletion {
                at: self.at,
                who: Who {
                    kind: self.actor,
                    person_id: self.actor_person,
                    module: self.actor_module,
                },
                before: self.before.0,
            },
        )
    }
}

/// The entry in the trash by this id, locked until the transaction ends so two
/// restores of it cannot both write it back.
pub(super) async fn deletion_for_update(transaction: &mut Transaction<'_, Postgres>, owner_id: Uuid, id: Uuid, keep_days: i32) -> Result<Option<Deletion>> {
    let row: Option<DeletionRow> = sqlx::query_as(concat!(deletions_select!(), " AND r.entry_id = $3 FOR UPDATE OF r"))
        .bind(owner_id)
        .bind(keep_days)
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await
        .context("finding the entry in the trash")?;
    Ok(row.map(|row| row.into_deletion().1))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use super::{Snapshot, changed};
    use crate::model::{CounterpartyRef, EntryField, EntryStatus, Line, LineSide, Tag};

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("a date")
    }

    fn line(amount: &str, side: LineSide) -> Line {
        Line {
            id: Uuid::new_v4(),
            side,
            account_id: None,
            category_id: None,
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: "PYG".to_owned(),
            note: String::new(),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            occurred_on: day(1),
            status: EntryStatus::Cleared,
            cleared_on: Some(day(1)),
            description: "lunch".to_owned(),
            source: None,
            idempotency_key: None,
            kept: false,
            created_at: Utc::now(),
            counterparty: None,
            place: None,
            tags: Vec::new(),
            lines: vec![line("-45000", LineSide::Account), line("45000", LineSide::Category)],
            attachments: Vec::new(),
        }
    }

    #[test]
    fn moving_a_posted_entry_to_another_day_changes_its_day_not_its_status() {
        // It was posted the day it happened, and still is.
        let after = Snapshot {
            occurred_on: day(2),
            cleared_on: Some(day(2)),
            ..snapshot()
        };
        assert_eq!(changed(&snapshot(), &after), vec![EntryField::OccurredOn]);

        let held = Snapshot {
            status: EntryStatus::Pending,
            cleared_on: None,
            ..snapshot()
        };
        let posted_later = Snapshot {
            cleared_on: Some(day(3)),
            ..snapshot()
        };
        assert_eq!(changed(&held, &posted_later), vec![EntryField::Status]);

        // Posted all along, on another day than it was said to be: a script
        // putting a statement's date right.
        let reposted = Snapshot {
            cleared_on: Some(day(5)),
            ..posted_later.clone()
        };
        assert_eq!(changed(&posted_later, &reposted), vec![EntryField::Status]);
    }

    #[test]
    fn a_part_renamed_since_is_the_same_part() {
        let tag = Uuid::new_v4();
        let counterparty = Uuid::new_v4();
        let before = Snapshot {
            tags: vec![Tag {
                id: tag,
                name: "trip".to_owned(),
            }],
            counterparty: Some(CounterpartyRef {
                id: counterparty,
                name: "Casa Rica".to_owned(),
            }),
            ..snapshot()
        };
        let after = Snapshot {
            tags: vec![Tag {
                id: tag,
                name: "holiday".to_owned(),
            }],
            counterparty: Some(CounterpartyRef {
                id: counterparty,
                name: "Casa Rica Centro".to_owned(),
            }),
            ..before.clone()
        };
        assert_eq!(changed(&before, &after), Vec::<EntryField>::new());
    }

    #[test]
    fn lines_written_again_for_the_same_money_are_the_same_lines() {
        // Posting a hold for exactly what was held rewrites its lines under
        // new ids and a scale of eighteen; the money is what it was.
        let before = snapshot();
        let after = Snapshot {
            lines: vec![line("-45000.000000000000000000", LineSide::Account), line("45000", LineSide::Category)],
            ..snapshot()
        };
        assert_eq!(changed(&before, &after), Vec::<EntryField>::new());

        let more = Snapshot {
            lines: vec![line("-50000", LineSide::Account), line("50000", LineSide::Category)],
            ..snapshot()
        };
        assert_eq!(changed(&before, &more), vec![EntryField::Lines]);
    }
}
