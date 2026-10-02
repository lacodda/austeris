//! Changing an entry after it was recorded: what it says about itself, and
//! posting it when it was held.

use austeris_common::{AppError, AppResult, Caller};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::{PlaceBody, Recorded, refused_or_missing, tag_names, today};
use crate::currency;
use crate::exchange::{self, Money, Summary};
use crate::model::{Entry, EntryStatus, LineSide, Purpose};
use crate::repository::{self, Change, Cleared, CounterpartyChoice, EntryChanges, NewLine, Side};

/// What changes about an entry. A field left out is left as it is; `null`
/// clears the counterparty or the place.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct EntryPatch {
    /// The day the money moved. A posted entry that was posted the same day
    /// moves with it.
    pub occurred_on: Option<NaiveDate>,
    /// What a person would call it.
    pub description: Option<String>,
    /// Who the money went to or came from; `null` for no one.
    #[serde(default)]
    #[schema(value_type = Option<Uuid>)]
    pub counterparty_id: Change<Uuid>,
    /// Where it happened; `null` for nowhere in particular.
    #[serde(default)]
    #[schema(value_type = Option<PlaceBody>)]
    pub place: Change<PlaceBody>,
    /// The labels, replacing every one it had; `[]` takes them all off.
    pub tags: Option<Vec<String>>,
}

/// Changes what an entry says about itself.
///
/// Its lines are not touched here: the money of an entry is changed by
/// posting it for a different amount, or by recording it again.
#[utoipa::path(
    patch,
    path = "/api/v1/ledger/entries/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    request_body = EntryPatch,
    responses(
        (status = 200, description = "The entry, as it now is", body = Entry),
        (status = 400, description = "A tag or a place is not one, or the day is after the one it was posted on", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such entry, or the counterparty is not yours", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn change_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(patch): Json<EntryPatch>) -> AppResult<Json<Entry>> {
    let changes = EntryChanges {
        occurred_on: patch.occurred_on,
        description: patch.description.map(|text| text.trim().to_owned()),
        counterparty: patch.counterparty_id.try_map(|id| Ok::<_, AppError>(CounterpartyChoice::Existing(id)))?,
        place: patch.place.try_map(|place| place.checked())?,
        tags: patch.tags.as_deref().map(tag_names).transpose()?,
    };

    repository::change_entry(&pool, caller.id(), id, &changes)
        .await
        .map_err(refused_or_missing)?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such entry")))
}

/// What posting a held entry carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ClearBody {
    /// The day the bank posted it. Today when omitted.
    pub on: Option<NaiveDate>,
    /// What the account actually moved by, in its own currency, when that is
    /// not what was held: a hotel's pre-authorisation, a fuel pump's, a
    /// charge in another currency at the bank's rate. Positive, like an amount
    /// typed in a line.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "180000")]
    pub amount: Option<Decimal>,
}

/// Posts a held entry.
///
/// With `amount`, the entry posts for that instead of what was held. A plain
/// payment's category moves with it. A payment in another currency keeps what
/// it cost in that currency, and the conversion between the two is worked out
/// again at the posted amount - with the fee measured against the rate on the
/// day it posted, which is the day the bank converted it.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/{id}/clear",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    request_body = ClearBody,
    responses(
        (status = 200, description = "The entry, posted, and the conversion when it was worked out again", body = Recorded),
        (status = 400, description = "The day is before it happened, or an entry of this shape cannot post for another amount", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
        (status = 409, description = "It was posted already", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn clear_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(body): Json<ClearBody>) -> AppResult<Json<Recorded>> {
    let owner_id = caller.id();
    let entry = repository::entry(&pool, owner_id, id)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such entry")))?;
    if let (EntryStatus::Cleared, Some(day)) = (entry.status, entry.cleared_on) {
        return Err(already_posted(day));
    }

    let on = body.on.unwrap_or_else(today);
    if on < entry.occurred_on {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "a payment posts on or after the day it was made, {}",
            entry.occurred_on
        )));
    }

    let (lines, conversion) = match body.amount {
        None => (None, None),
        Some(amount) => {
            let (lines, conversion) = reposted(&pool, owner_id, &entry, amount, on).await?;
            (Some(lines), conversion)
        }
    };

    match repository::clear_entry(&pool, owner_id, id, on, lines.as_deref())
        .await
        .map_err(refused_or_missing)?
    {
        Cleared::Now(entry) => Ok(Json(Recorded {
            entry: *entry,
            conversion,
            new_counterparty: None,
            new_tags: Vec::new(),
        })),
        // Posted by another request since it was read.
        Cleared::Already(day) => Err(already_posted(day)),
        Cleared::Missing => Err(AppError::not_found(anyhow::anyhow!("no such entry"))),
    }
}

fn already_posted(day: NaiveDate) -> AppError {
    AppError::new(StatusCode::CONFLICT, anyhow::anyhow!("this entry was posted on {day} already"))
}

/// The lines of a held entry posted for `amount` rather than what was held.
///
/// Only an entry moving one account can: with two, which of them the amount
/// is for is a guess. And only one whose other side is one line, or a
/// conversion - a split receipt posted for a different total does not say
/// which of its parts changed.
async fn reposted(pool: &PgPool, owner_id: Uuid, entry: &Entry, amount: Decimal, on: NaiveDate) -> AppResult<(Vec<NewLine>, Option<Summary>)> {
    if !amount.is_sign_positive() || amount.is_zero() {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "the amount posted is positive, like an amount typed in a line"
        )));
    }

    let accounts: Vec<_> = entry.lines.iter().filter(|line| line.side == LineSide::Account).collect();
    let [account_line] = accounts.as_slice() else {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "this entry moves {} accounts, so which of them posted for another amount is not said; record it again instead",
            accounts.len()
        )));
    };
    let account = account_line
        .account_id
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("an account line without an account")))?;
    let held_in = account_line.currency.clone();
    if currency::round(amount, &held_in) != amount {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "{amount} is not an amount of {held_in}: it has more decimal places than {held_in} does"
        )));
    }
    // The account moves the way it was held to: out for a payment, in for a
    // refund.
    let signed = if account_line.amount.is_sign_negative() { -amount } else { amount };
    let account_side = NewLine {
        side: Side::Account(account),
        amount: signed,
        currency: held_in.clone(),
        note: account_line.note.clone(),
    };

    let converts = entry.lines.iter().any(|line| line.side == LineSide::Conversion);
    if !converts {
        let others: Vec<_> = entry.lines.iter().filter(|line| line.side != LineSide::Account).collect();
        let [other] = others.as_slice() else {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "a split entry does not say which of its parts posted for another amount; record it again instead"
            )));
        };
        let other_side = match (other.side, other.category_id) {
            (LineSide::Category, Some(category)) => Side::Category(category),
            _ => {
                return Err(AppError::bad_request(anyhow::anyhow!(
                    "only a payment to a category posts for another amount; record a transfer again instead"
                )));
            }
        };
        let lines = vec![
            account_side,
            NewLine {
                side: other_side,
                amount: -signed,
                currency: held_in,
                note: other.note.clone(),
            },
        ];
        return Ok((lines, None));
    }

    reconverted(pool, owner_id, entry, account_side, amount, on).await
}

/// The lines of a held payment in another currency, posted for `amount`.
///
/// What it cost in the other currency is kept, and what lies between that and
/// the account is worked out again. The old fee is part of what is worked out
/// again, so it is told apart from the categories kept.
async fn reconverted(
    pool: &PgPool,
    owner_id: Uuid,
    entry: &Entry,
    account_side: NewLine,
    amount: Decimal,
    on: NaiveDate,
) -> AppResult<(Vec<NewLine>, Option<Summary>)> {
    let held_in = account_side.currency.clone();
    let signed = account_side.amount;
    let fee_category = repository::categories(pool, owner_id)
        .await?
        .into_iter()
        .find(|category| category.purpose == Some(Purpose::ExchangeFees))
        .map(|category| category.id);
    let kept: Vec<NewLine> = entry
        .lines
        .iter()
        .filter(|line| line.side == LineSide::Category && line.category_id.is_some() && line.category_id != fee_category)
        .map(|line| NewLine {
            side: Side::Category(line.category_id.unwrap_or_default()),
            amount: line.amount,
            currency: line.currency.clone(),
            note: line.note.clone(),
        })
        .collect();
    let other_currency = match kept.first() {
        Some(line) if kept.iter().all(|other| other.currency == line.currency) && line.currency != held_in => line.currency.clone(),
        _ => {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "this conversion's categories are not in one other currency; record it again instead"
            )));
        }
    };
    let cost: Decimal = kept.iter().map(|line| line.amount).sum::<Decimal>().abs();

    let (given, got) = if signed.is_sign_negative() {
        (
            Money {
                amount,
                currency: held_in.clone(),
            },
            Money {
                amount: cost,
                currency: other_currency,
            },
        )
    } else {
        (
            Money {
                amount: cost,
                currency: other_currency,
            },
            Money {
                amount,
                currency: held_in.clone(),
            },
        )
    };
    let reference = repository::rate_in_force(pool, &got.currency, &given.currency, on).await?;
    let plan = exchange::plan(given, got, reference).map_err(AppError::bad_request)?;
    let fees = if plan.has_fee() {
        Some(
            repository::purpose_category(pool, owner_id, Purpose::ExchangeFees)
                .await
                .map_err(AppError::bad_request)?
                .id,
        )
    } else {
        None
    };

    let mut lines = vec![account_side];
    lines.extend(plan.bridge(fees).map_err(AppError::internal)?);
    lines.extend(kept);
    Ok((lines, Some(plan.summary)))
}
