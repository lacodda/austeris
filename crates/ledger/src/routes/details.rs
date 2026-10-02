//! Counterparties, places and tags, and the totals they gather money into.

use austeris_common::{AppError, AppResult, Caller};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use super::{PlaceBody, conflict_or, is_foreign_key_violation, is_still_referenced, is_unique_violation};
use crate::model::{Counterparty, CounterpartyKind, Group, GroupBy, Place, Tag};
use crate::repository::{self, Change, CounterpartyChanges, TotalsFilter};

#[utoipa::path(
    get,
    path = "/api/v1/ledger/counterparties",
    tag = "ledger",
    responses((status = 200, description = "Every counterparty, by name", body = Vec<Counterparty>)),
)]
pub async fn list_counterparties(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Counterparty>>> {
    Ok(Json(repository::counterparties(&pool, caller.id()).await?))
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/counterparties/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The counterparty")),
    responses(
        (status = 200, description = "The counterparty", body = Counterparty),
        (status = 404, description = "No such counterparty", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn read_counterparty(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Counterparty>> {
    repository::counterparty(&pool, caller.id(), id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such counterparty")))
}

/// What creating a counterparty carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewCounterparty {
    /// What to call it.
    pub name: String,
    /// What it is; left unsaid when omitted.
    pub kind: Option<CounterpartyKind>,
    /// What money spent there is usually for.
    pub default_category_id: Option<Uuid>,
}

/// Creates a counterparty.
///
/// Naming one after `@` in a typed line creates it too; this is for one
/// recorded before it is first paid, or with its kind said from the start.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/counterparties",
    tag = "ledger",
    request_body = NewCounterparty,
    responses(
        (status = 201, description = "The counterparty", body = Counterparty),
        (status = 400, description = "A counterparty needs a name with a letter or a digit in it", body = austeris_common::error::ErrorBody),
        (status = 404, description = "The category is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "You have one typed the same way", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn create_counterparty(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewCounterparty>) -> AppResult<(StatusCode, Json<Counterparty>)> {
    let name = counterparty_name(&new.name)?;
    let counterparty = repository::create_counterparty(&pool, caller.id(), &name, new.kind, new.default_category_id)
        .await
        .map_err(|error| counterparty_refused(error, &name))?;
    Ok((StatusCode::CREATED, Json(counterparty)))
}

/// What changes about a counterparty. A field left out is left as it is;
/// `null` clears the kind or the usual category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CounterpartyPatch {
    /// What to call it.
    pub name: Option<String>,
    /// What it is.
    #[serde(default)]
    #[schema(value_type = Option<CounterpartyKind>)]
    pub kind: Change<CounterpartyKind>,
    /// What money spent there is usually for.
    #[serde(default)]
    #[schema(value_type = Option<Uuid>)]
    pub default_category_id: Change<Uuid>,
}

/// Renames a counterparty, says what it is, or changes what money spent there
/// is usually for.
///
/// Every entry naming it follows: they name the counterparty, not a copy of its
/// name.
#[utoipa::path(
    patch,
    path = "/api/v1/ledger/counterparties/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The counterparty")),
    request_body = CounterpartyPatch,
    responses(
        (status = 200, description = "The counterparty, as it now is", body = Counterparty),
        (status = 404, description = "No such counterparty, or the category is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "You have another one typed the same way", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn change_counterparty(
    State(pool): State<PgPool>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(patch): Json<CounterpartyPatch>,
) -> AppResult<Json<Counterparty>> {
    let name = patch.name.as_deref().map(counterparty_name).transpose()?;
    let changes = CounterpartyChanges {
        name: name.clone(),
        kind: patch.kind,
        default_category_id: patch.default_category_id,
    };
    repository::change_counterparty(&pool, caller.id(), id, &changes)
        .await
        .map_err(|error| counterparty_refused(error, name.as_deref().unwrap_or_default()))?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such counterparty")))
}

/// Deletes a counterparty no entry names.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/counterparties/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The counterparty")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such counterparty", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Entries name it", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn delete_counterparty(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    match repository::delete_counterparty(&pool, caller.id(), id).await {
        Ok(true) => Ok(StatusCode::NO_CONTENT),
        Ok(false) => Err(AppError::not_found(anyhow::anyhow!("no such counterparty"))),
        // Deleting it would leave its entries saying nothing about who they
        // were with, which is a change to the books nobody asked for.
        Err(error) if is_still_referenced(&error) => Err(AppError::new(
            StatusCode::CONFLICT,
            anyhow::anyhow!("entries name this counterparty; change them first"),
        )),
        Err(error) => Err(AppError::internal(error)),
    }
}

/// A counterparty's name, trimmed and not empty.
fn counterparty_name(raw: &str) -> AppResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("a counterparty needs a name")));
    }
    Ok(name.to_owned())
}

/// What a refused write of a counterparty means to the caller.
fn counterparty_refused(error: anyhow::Error, name: &str) -> AppError {
    if is_unique_violation(&error) {
        return AppError::new(StatusCode::CONFLICT, anyhow::anyhow!("you have a counterparty typed the same way as `{name}`"));
    }
    if is_foreign_key_violation(&error) {
        return AppError::not_found(anyhow::anyhow!("you have no such category"));
    }
    // The schema's own rule: a name of nothing but punctuation cannot be typed.
    let refused = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .any(|error| error.as_database_error().is_some_and(|db| db.code().as_deref() == Some("23514")));
    if refused {
        return AppError::bad_request(anyhow::anyhow!("`{name}` has no letter or digit in it, so it could never be typed"));
    }
    AppError::internal(error)
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/places",
    tag = "ledger",
    responses((status = 200, description = "Every place, by country and city", body = Vec<Place>)),
)]
pub async fn list_places(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Place>>> {
    Ok(Json(repository::places(&pool, caller.id()).await?))
}

/// Renames a place - every entry that happened there follows.
#[utoipa::path(
    patch,
    path = "/api/v1/ledger/places/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The place")),
    request_body = PlaceBody,
    responses(
        (status = 200, description = "The place, as it now is", body = Place),
        (status = 400, description = "The country is not a code", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such place", body = austeris_common::error::ErrorBody),
        (status = 409, description = "You have that place already", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn change_place(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(body): Json<PlaceBody>) -> AppResult<Json<Place>> {
    let place = body.checked()?;
    repository::change_place(&pool, caller.id(), id, &place)
        .await
        .map_err(|error| conflict_or(error, "you have that place already"))?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such place")))
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/tags",
    tag = "ledger",
    responses((status = 200, description = "Every tag, by name", body = Vec<Tag>)),
)]
pub async fn list_tags(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Tag>>> {
    Ok(Json(repository::tags(&pool, caller.id()).await?))
}

/// What renaming a tag carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TagRename {
    /// The new name: one word starting with a letter.
    pub name: String,
}

/// Renames a tag on every entry that carries it.
#[utoipa::path(
    patch,
    path = "/api/v1/ledger/tags/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The tag")),
    request_body = TagRename,
    responses(
        (status = 200, description = "The tag, as it now is", body = Tag),
        (status = 400, description = "That is not a tag", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such tag", body = austeris_common::error::ErrorBody),
        (status = 409, description = "You have a tag by that name", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn rename_tag(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(body): Json<TagRename>) -> AppResult<Json<Tag>> {
    let names = super::tag_names(std::slice::from_ref(&body.name))?;
    let name = names.first().ok_or_else(|| AppError::bad_request(anyhow::anyhow!("a tag needs a name")))?;
    repository::rename_tag(&pool, caller.id(), id, name)
        .await
        .map_err(|error| conflict_or(error, "you have a tag by that name"))?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such tag")))
}

/// Deletes a tag, taking it off every entry that carried it. The entries stay.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/tags/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The tag")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such tag", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn delete_tag(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::delete_tag(&pool, caller.id(), id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such tag")))
    }
}

/// Which totals are asked for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TotalsQuery {
    /// What to gather by: `category`, `tag`, `counterparty`, `place` or
    /// `country`.
    pub by: GroupBy,
    /// Only entries on or after this day.
    pub from: Option<NaiveDate>,
    /// Only entries on or before this day.
    pub to: Option<NaiveDate>,
    /// Only entries carrying this tag - with `by=category`, what it went on.
    pub tag: Option<String>,
    /// Only entries with this counterparty.
    pub counterparty: Option<Uuid>,
    /// Only entries moving this account.
    pub account: Option<Uuid>,
}

/// What money was spent on and earned from, gathered.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Totals {
    /// What it is gathered by.
    pub by: GroupBy,
    /// The first day counted, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<NaiveDate>,
    /// The last day counted, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<NaiveDate>,
    /// One per category, tag, counterparty, place or country, by name; the
    /// entries that name none come last, as a group without a name.
    pub groups: Vec<Group>,
}

/// What money was spent on and earned from, by category, tag, counterparty,
/// place or country.
///
/// Read from the lines on categories, which is what money is for: a transfer
/// between your own accounts moves nothing here. Each group says what was spent
/// and earned in each currency, never added across them. Held payments count
/// on the day they were made.
///
/// A tag's total over all time is what it has gathered so far - `#holiday-2027`
/// as it stands; the same with `by=category&tag=holiday-2027` is what it went
/// on.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/totals",
    tag = "ledger",
    params(TotalsQuery),
    responses(
        (status = 200, description = "The groups, each in every currency it moved in", body = Totals),
        (status = 400, description = "The window ends before it starts", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn totals(State(pool): State<PgPool>, caller: Caller, Query(query): Query<TotalsQuery>) -> AppResult<Json<Totals>> {
    if let (Some(from), Some(to)) = (query.from, query.to)
        && from > to
    {
        return Err(AppError::bad_request(anyhow::anyhow!("the window ends before it starts")));
    }

    let filter = TotalsFilter {
        from: query.from,
        to: query.to,
        tag: query.tag.as_deref().map(|tag| tag.trim().trim_start_matches('#').to_owned()),
        counterparty_id: query.counterparty,
        account_id: query.account,
    };
    let groups = repository::totals(&pool, caller.id(), query.by, &filter).await?;
    Ok(Json(Totals {
        by: query.by,
        from: query.from,
        to: query.to,
        groups,
    }))
}
