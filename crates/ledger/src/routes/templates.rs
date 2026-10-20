//! Recording what was recorded before: an entry again, or a template's button.
//!
//! Both start from an entry the person already wrote. A template is made from
//! one - "make this a button" - rather than filled in as a form: the entry
//! already says which account, category, counterparty and tags, and a person
//! recognises a coffee they bought more readily than they describe one.

use std::str::FromStr;

use austeris_common::{AppError, AppResult, Caller, Code};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::payment::Payment;
use super::{Recorded, conflict_or, refused_or_missing, today};
use crate::model::{Entry, LineSide, Purpose, Template};
use crate::paid;
use crate::repository::{self, Change, CounterpartyChoice, NewEntry, NewLine, NewPlace, NoTemplate, Side, TemplateChanges};

/// What making a template carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewTemplate {
    /// The entry to make it from: one payment from one account on one
    /// category.
    pub entry_id: Uuid,
    /// What the button says.
    pub name: String,
    /// Ask for the amount each time instead of keeping the entry's: the same
    /// shop, a different receipt.
    #[serde(default)]
    pub ask_amount: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/templates",
    tag = "ledger",
    responses((status = 200, description = "Every template, in the order the buttons sit", body = Vec<Template>)),
)]
pub async fn list_templates(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Template>>> {
    Ok(Json(repository::templates(&pool, caller.id()).await?))
}

/// Makes a button out of an entry.
///
/// The entry has to be one payment: one account, one category, and the
/// conversion between them when it was charged in another currency. A split
/// receipt, a transfer and an exchange are recorded again instead - which of
/// their parts a button would mean is a guess.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/templates",
    tag = "ledger",
    request_body = NewTemplate,
    responses(
        (status = 201, description = "The template, as the last button", body = Template),
        (status = 400, description = "It has no name, or the entry is not one payment", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
        (status = 409, description = "There is already a template by that name", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn create_template(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewTemplate>) -> AppResult<(StatusCode, Json<Template>)> {
    let owner_id = caller.id();
    let name = new.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("a template needs a name")));
    }
    let template = repository::template_from_entry(&pool, owner_id, new.entry_id, name, new.ask_amount)
        .await
        .map_err(|error| match error.downcast_ref::<NoTemplate>() {
            Some(NoTemplate::NoEntry) => AppError::not_found(anyhow::anyhow!("no such entry")),
            Some(not_one @ NoTemplate::NotOnePayment(_)) => AppError::bad_request(anyhow::anyhow!("{not_one}")),
            None => name_taken_or(error),
        })?;
    Ok((StatusCode::CREATED, Json(template)))
}

/// What changes about a template. A field left out is left as it is.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TemplatePatch {
    /// What the button says.
    pub name: Option<String>,
    /// How much, as a decimal string; `null` makes it ask each time.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "15000")]
    pub amount: Change<String>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/ledger/templates/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The template")),
    request_body = TemplatePatch,
    responses(
        (status = 200, description = "The template, as it now is", body = Template),
        (status = 400, description = "The name is empty, or the amount is not a positive one", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such template", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Another template has that name", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn change_template(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(patch): Json<TemplatePatch>) -> AppResult<Json<Template>> {
    let name = match patch.name.as_deref().map(str::trim) {
        Some("") => return Err(AppError::bad_request(anyhow::anyhow!("a template needs a name"))),
        other => other.map(ToOwned::to_owned),
    };
    let changes = TemplateChanges {
        name,
        amount: patch.amount.try_map(|text| positive(&text))?,
    };
    repository::change_template(&pool, caller.id(), id, &changes)
        .await
        .map_err(name_taken_or)?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such template")))
}

/// The order a person's buttons sit in.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TemplateOrder {
    /// Every template of the person's, each once, first button first.
    pub ids: Vec<Uuid>,
}

#[utoipa::path(
    put,
    path = "/api/v1/ledger/templates/order",
    tag = "ledger",
    request_body = TemplateOrder,
    responses(
        (status = 204, description = "Ordered"),
        (status = 400, description = "The ids are not every template of yours, each once", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn order_templates(State(pool): State<PgPool>, caller: Caller, Json(order): Json<TemplateOrder>) -> AppResult<StatusCode> {
    if repository::order_templates(&pool, caller.id(), &order.ids).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::bad_request(anyhow::anyhow!("an order names every template of yours, each once")))
    }
}

/// Deletes a template. The entries it recorded stay.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/templates/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The template")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such template", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn delete_template(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::delete_template(&pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such template")))
    }
}

/// What pressing a template's button carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UseTemplate {
    /// How much, in the template's currency, as a decimal string. Needed when
    /// the template asks for it; otherwise it replaces the template's own for
    /// this once.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "52000")]
    pub amount: Option<Decimal>,
    /// The day the money moved. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
    /// Record it even when the same entry was recorded a moment ago.
    #[serde(default)]
    pub allow_duplicate: bool,
}

/// Records the template's payment.
///
/// An amount in another currency than the account's is converted at the
/// day's rate, as a typed line without `@` is. A second press within a
/// minute is refused like any other entry recorded twice.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/templates/{id}/use",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The template")),
    request_body = UseTemplate,
    responses(
        (status = 201, description = "The entry, and the conversion when there was one", body = Recorded),
        (status = 400, description = "The template asks for an amount and none was given, or no rate is known for its currency", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such template", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The same entry was recorded a moment ago", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn use_template(
    State(pool): State<PgPool>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<UseTemplate>,
) -> AppResult<(StatusCode, Json<Recorded>)> {
    let owner_id = caller.id();
    let template = repository::template(&pool, owner_id, id)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such template")))?;
    let amount = match (body.amount, template.amount) {
        (Some(given), _) => {
            if !given.is_sign_positive() || given.is_zero() {
                return Err(AppError::bad_request(anyhow::anyhow!("an amount is positive")));
            }
            given
        }
        (None, Some(kept)) => kept,
        (None, None) => {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "`{}` asks for the amount each time; say how much",
                template.name
            )));
        }
    };

    let account = repository::account(&pool, owner_id, template.account_id)
        .await?
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("a template's account vanished")))?;
    let category = repository::categories(&pool, owner_id)
        .await?
        .into_iter()
        .find(|category| category.id == template.category_id)
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("a template's category vanished")))?;

    let payment = Payment {
        occurred_on: body.occurred_on.unwrap_or_else(today),
        flow: category.flow,
        amount,
        currency: template.currency,
        rate: None,
        note: template.description.clone(),
        description: template.description,
        counterparty: template.counterparty.map(|known| CounterpartyChoice::Existing(known.id)),
        tags: template.tags,
        place: template.place.map(|place| NewPlace {
            country: place.country,
            city: place.city,
        }),
        pending: template.pending,
        account,
        category,
    };
    let (entry, conversion) = payment.priced(&pool).await?.entry(&pool, owner_id, body.allow_duplicate).await?;
    let posted = repository::post_entry(&pool, owner_id, &caller.into(), &entry)
        .await
        .map_err(refused_or_missing)?;
    Ok((StatusCode::CREATED, Json(recorded(posted.entry, conversion))))
}

/// What recording an entry again carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Repeat {
    /// The day the money moved this time. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
    /// Record it even when the same entry was recorded a moment ago.
    #[serde(default)]
    pub allow_duplicate: bool,
}

/// Records an entry again: yesterday's bus fare, today.
///
/// The copy says what the entry says - its lines, description, counterparty,
/// place and tags - on another day. It is held when the entry went through a
/// hold: a payment that was held once is the kind that is.
///
/// A payment charged in another currency keeps what it cost there and is
/// converted again at the day's rate: the rate is a fact of the day, and
/// yesterday's would be a number nobody was charged. An exchange between two
/// accounts is refused - what changes hands is the deal, and the next one is
/// not the same.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/{id}/repeat",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry to record again")),
    request_body = Repeat,
    responses(
        (status = 201, description = "The new entry, and the conversion when it was converted again", body = Recorded),
        (status = 400, description = "The entry is an exchange, or no rate is known for its currency", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The same entry was recorded a moment ago", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn repeat(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(body): Json<Repeat>) -> AppResult<(StatusCode, Json<Recorded>)> {
    let owner_id = caller.id();
    let entry = repository::entry(&pool, owner_id, id)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such entry")))?;
    let occurred_on = body.occurred_on.unwrap_or_else(today);
    let pending = repository::was_held(entry.occurred_on, entry.cleared_on);
    let counterparty = entry.counterparty.as_ref().map(|known| CounterpartyChoice::Existing(known.id));
    let place = entry.place.as_ref().map(|place| NewPlace {
        country: place.country.clone(),
        city: place.city.clone(),
    });

    let converts = entry.lines.iter().any(|line| line.side == LineSide::Conversion);
    let (new, conversion) = if converts {
        let fee_category = repository::category_for(&pool, owner_id, Purpose::ExchangeFees)
            .await?
            .map(|category| category.id);
        let paid = paid::of(&entry, fee_category)
            .map_err(|why| AppError::bad_request(anyhow::anyhow!("{why}; an exchange is recorded anew with what changed hands")))?;
        let account = repository::account(&pool, owner_id, paid.account_id)
            .await?
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("an entry's account vanished")))?;
        let category = repository::categories(&pool, owner_id)
            .await?
            .into_iter()
            .find(|category| category.id == paid.category_id)
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("an entry's category vanished")))?;
        let payment = Payment {
            occurred_on,
            flow: paid.flow,
            amount: paid.amount,
            currency: paid.currency,
            rate: None,
            description: entry.description.clone(),
            note: paid.note,
            counterparty,
            tags: entry.tags.clone(),
            place,
            pending,
            account,
            category,
        };
        payment.priced(&pool).await?.entry(&pool, owner_id, body.allow_duplicate).await?
    } else {
        // In one currency the lines are the entry, and they say the same
        // tomorrow: a split receipt and a transfer are copied as they are.
        let lines = entry
            .lines
            .iter()
            .filter_map(|line| {
                let side = match (line.side, line.account_id, line.category_id) {
                    (LineSide::Account, Some(account), _) => Side::Account(account),
                    (LineSide::Category, _, Some(category)) => Side::Category(category),
                    _ => return None,
                };
                Some(NewLine {
                    side,
                    amount: line.amount,
                    currency: line.currency.clone(),
                    note: line.note.clone(),
                })
            })
            .collect();
        let new = NewEntry {
            occurred_on,
            description: entry.description.clone(),
            idempotency_key: None,
            source: None,
            pending,
            counterparty,
            place,
            tags: entry.tags.clone(),
            lines,
            files: Vec::new(),
            allow_duplicate: body.allow_duplicate,
        };
        (new, None)
    };

    let posted = repository::post_entry(&pool, owner_id, &caller.into(), &new)
        .await
        .map_err(refused_or_missing)?;
    Ok((StatusCode::CREATED, Json(recorded(posted.entry, conversion))))
}

/// What a template or a repeat answers with: the entry and its conversion.
fn recorded(entry: Entry, conversion: Option<crate::exchange::Summary>) -> Recorded {
    Recorded {
        entry,
        conversion,
        new_counterparty: None,
        new_tags: Vec::new(),
    }
}

/// A name another of the person's buttons has, said with its code; any other
/// failure as it is.
fn name_taken_or(error: anyhow::Error) -> AppError {
    let refused = conflict_or(error, "there is already a template by that name");
    if refused.status() == StatusCode::CONFLICT {
        refused.with_code(Code::TemplateNameTaken, serde_json::json!({}))
    } else {
        refused
    }
}

/// A positive amount, from the decimal string a client sent.
fn positive(text: &str) -> AppResult<Decimal> {
    let amount = Decimal::from_str(text.trim()).map_err(|_| AppError::bad_request(anyhow::anyhow!("`{text}` is not an amount")))?;
    if !amount.is_sign_positive() || amount.is_zero() {
        return Err(AppError::bad_request(anyhow::anyhow!("an amount is positive")));
    }
    Ok(amount)
}
