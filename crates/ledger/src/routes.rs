//! The ledger service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards `/api/v1/ledger/...`
//! here (ADR 0001). Every handler is scoped by [`Caller`] - the person the
//! gateway vouched for - and there is no path that reads across people.

use austeris_common::{AppError, AppResult, Caller, Code, health};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::exchange::{self, Money, Summary};
use crate::model::{
    Account, AccountKind, Attachment, Balance, Category, Counterparty, CounterpartyKind, CounterpartyRef, Entry, EntryField, EntryStatus, ExchangeRate, Flow,
    Flowed, Group, GroupBy, Line, LineSide, Place, Purpose, RateInForce, Revision, RevisionAction, Tag, Template, Trash, TrashedAttachment, TrashedEntry, Who,
    WhoKind,
};
use crate::repository::{CounterpartyChoice, EntryFilter, KeptEntry, NewEntry, NewLine, NewPlace, PossibleDuplicate, Side};
use crate::{MIGRATOR, attachment, balance, country, parse, repository};

mod attachments;
mod details;
mod history;
mod holds;
mod payment;
mod quick;
mod templates;

/// Builds the service's router.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool.clone(), &MIGRATOR))))
        .route("/ledger/accounts", get(list_accounts).post(create_account))
        .route("/ledger/accounts/{id}", get(read_account))
        .route("/ledger/accounts/{id}/close", post(close_account))
        .route("/ledger/categories", get(list_categories).post(create_category))
        .route("/ledger/entries", get(list_entries).post(create_entry))
        .route("/ledger/entries/quick", post(quick::quick_entry))
        .route("/ledger/entries/quick/preview", post(quick::preview))
        .route("/ledger/entries/{id}", get(read_entry).patch(holds::change_entry).delete(delete_entry))
        .route("/ledger/entries/{id}/clear", post(holds::clear_entry))
        .route("/ledger/entries/{id}/repeat", post(templates::repeat))
        .route("/ledger/entries/{id}/history", get(history::history))
        .route(
            "/ledger/entries/{id}/attachments",
            post(attachments::attach).layer(DefaultBodyLimit::max(attachment::BODY_LIMIT)),
        )
        .route("/ledger/attachments/{id}", delete(attachments::remove))
        .route("/ledger/attachments/{id}/content", get(attachments::content))
        .route("/ledger/trash", get(history::trash).delete(history::empty))
        .route("/ledger/trash/entries/{id}", delete(history::erase_entry))
        .route("/ledger/trash/entries/{id}/restore", post(history::restore_entry))
        .route("/ledger/trash/attachments/{id}", delete(history::erase_attachment))
        .route("/ledger/trash/attachments/{id}/restore", post(history::restore_attachment))
        .route("/ledger/templates", get(templates::list_templates).post(templates::create_template))
        .route("/ledger/templates/order", put(templates::order_templates))
        .route("/ledger/templates/{id}", patch(templates::change_template).delete(templates::delete_template))
        .route("/ledger/templates/{id}/use", post(templates::use_template))
        .route("/ledger/exchanges", post(exchange))
        .route("/ledger/balances", get(balances))
        .route("/ledger/totals", get(details::totals))
        .route("/ledger/counterparties", get(details::list_counterparties).post(details::create_counterparty))
        .route(
            "/ledger/counterparties/{id}",
            get(details::read_counterparty)
                .patch(details::change_counterparty)
                .delete(details::delete_counterparty),
        )
        .route("/ledger/places", get(details::list_places))
        .route("/ledger/places/{id}", patch(details::change_place))
        .route("/ledger/tags", get(details::list_tags))
        .route("/ledger/tags/{id}", patch(details::rename_tag).delete(details::delete_tag))
        .route("/ledger/rates", get(list_rates).post(record_rate))
        .route("/ledger/rates/at", get(rate_at))
        .with_state(pool)
}

/// What creating an account carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewAccount {
    /// What kind of place this is.
    pub kind: AccountKind,
    /// What to call it.
    pub name: String,
    /// The currency it is denominated in.
    pub currency: String,
    /// What was in it before austeris knew about it.
    #[serde(default, with = "rust_decimal::serde::str_option")]
    #[schema(value_type = Option<String>, example = "1250.00")]
    pub opening_balance: Option<Decimal>,
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/accounts",
    tag = "ledger",
    responses((status = 200, description = "Every account, open ones first", body = Vec<Account>)),
)]
async fn list_accounts(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Account>>> {
    Ok(Json(repository::accounts(&pool, caller.id()).await?))
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/accounts/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The account")),
    responses(
        (status = 200, description = "The account", body = Account),
        (status = 404, description = "No such account", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_account(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Account>> {
    repository::account(&pool, caller.id(), id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account")))
}

#[utoipa::path(
    post,
    path = "/api/v1/ledger/accounts",
    tag = "ledger",
    request_body = NewAccount,
    responses(
        (status = 201, description = "The account", body = Account),
        (status = 400, description = "An account needs a name and a currency", body = austeris_common::error::ErrorBody),
        (status = 409, description = "There is already an account by that name", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_account(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewAccount>) -> AppResult<(StatusCode, Json<Account>)> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("an account needs a name")));
    }
    let currency = currency_code(&new.currency)?;

    let account = repository::create_account(&pool, caller.id(), new.kind, name, &currency, new.opening_balance.unwrap_or(Decimal::ZERO))
        .await
        .map_err(|error| conflict_or(error, "there is already an account by that name"))?;

    Ok((StatusCode::CREATED, Json(account)))
}

/// Whether an account is being closed or reopened.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Closing {
    /// `true` closes it, `false` reopens it.
    #[serde(default = "yes")]
    pub closed: bool,
}

fn yes() -> bool {
    true
}

/// Closes an account, or reopens one.
///
/// Closed, never deleted: its entries are still true, and a report of last year
/// must not change because an account was shut this morning.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/accounts/{id}/close",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The account")),
    request_body = Closing,
    responses(
        (status = 200, description = "The account", body = Account),
        (status = 404, description = "No such account", body = austeris_common::error::ErrorBody),
    ),
)]
async fn close_account(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(closing): Json<Closing>) -> AppResult<Json<Account>> {
    repository::set_account_closed(&pool, caller.id(), id, closing.closed)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account")))
}

/// What creating a category carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewCategory {
    /// The category this one sits under; omitted makes it a root.
    pub parent_id: Option<Uuid>,
    /// Whether this is money in or out.
    pub flow: Flow,
    /// What to call it.
    pub name: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/categories",
    tag = "ledger",
    responses((status = 200, description = "Every category", body = Vec<Category>)),
)]
async fn list_categories(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Vec<Category>>> {
    Ok(Json(repository::categories(&pool, caller.id()).await?))
}

#[utoipa::path(
    post,
    path = "/api/v1/ledger/categories",
    tag = "ledger",
    request_body = NewCategory,
    responses(
        (status = 201, description = "The category", body = Category),
        (status = 400, description = "A category needs a name", body = austeris_common::error::ErrorBody),
        (status = 404, description = "No such parent", body = austeris_common::error::ErrorBody),
        (status = 409, description = "A sibling already has that name", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_category(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewCategory>) -> AppResult<(StatusCode, Json<Category>)> {
    let name = new.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(anyhow::anyhow!("a category needs a name")));
    }

    // A parent belonging to someone else would be accepted by the foreign key
    // and put this person's category in another person's tree.
    if let Some(parent_id) = new.parent_id {
        let parents = repository::categories(&pool, caller.id()).await?;
        if !parents.iter().any(|category| category.id == parent_id) {
            return Err(AppError::not_found(anyhow::anyhow!("no such parent category")));
        }
    }

    let category = repository::create_category(&pool, caller.id(), new.parent_id, new.flow, name)
        .await
        .map_err(|error| conflict_or(error, "a category by that name is already here"))?;

    Ok((StatusCode::CREATED, Json(category)))
}

/// One side of an entry, as a client states it.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewEntryLine {
    /// The account this side moves. Exactly one of this, `category_id` and
    /// `conversion`.
    pub account_id: Option<Uuid>,
    /// The category this side is attributed to.
    pub category_id: Option<Uuid>,
    /// This line is money changing currency inside the entry. An entry's
    /// conversion lines take one currency in and give another out.
    #[serde(default)]
    pub conversion: bool,
    /// The signed amount, as a decimal string: negative leaves, positive
    /// arrives (ADR 0004).
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "-45000.00")]
    pub amount: Decimal,
    /// What this line is in.
    pub currency: String,
    /// What this side was for.
    #[serde(default)]
    pub note: String,
}

/// What recording an entry carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewEntryBody {
    /// The day the money moved. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
    /// What a person would call it.
    #[serde(default)]
    pub description: String,
    /// The payment is held: the bank has not posted it yet. Posted the day it
    /// happened when omitted.
    #[serde(default)]
    pub pending: bool,
    /// Who the money went to or came from.
    pub counterparty_id: Option<Uuid>,
    /// Where it happened.
    pub place: Option<PlaceBody>,
    /// Labels, by name; one used for the first time is created.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The sides of the movement. At least two, summing to zero in every
    /// currency they touch.
    pub lines: Vec<NewEntryLine>,
    /// Record it even when the same entry was recorded a moment ago.
    #[serde(default)]
    pub allow_duplicate: bool,
}

/// A place, as a client states it.
#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct PlaceBody {
    /// ISO 3166-1 alpha-2, in either case: `PY`, `br`.
    pub country: String,
    /// The city, when there is one.
    pub city: Option<String>,
}

impl PlaceBody {
    /// The place, with its country checked against the codes that exist and
    /// its city trimmed - an empty one is the country as a whole.
    fn checked(&self) -> AppResult<NewPlace> {
        let country = country::code(&self.country).ok_or_else(|| {
            AppError::bad_request(anyhow::anyhow!(
                "`{}` is not a country code; two letters from ISO 3166-1, as in PY or BR (the United Kingdom is GB)",
                self.country
            ))
        })?;
        let city = self.city.as_deref().map(str::trim).filter(|city| !city.is_empty()).map(ToOwned::to_owned);
        Ok(NewPlace { country, city })
    }
}

/// Tag names as a client sent them, each checked and each once.
fn tag_names(names: &[String]) -> AppResult<Vec<String>> {
    let mut out: Vec<String> = Vec::with_capacity(names.len());
    for raw in names {
        let name = parse::tag_name(raw.trim().trim_start_matches('#')).ok_or_else(|| {
            AppError::bad_request(anyhow::anyhow!(
                "`{raw}` is not a tag; a tag is one word starting with a letter, as in `holiday-2027`"
            ))
        })?;
        if !out.iter().any(|seen| seen.to_lowercase() == name.to_lowercase()) {
            out.push(name);
        }
    }
    Ok(out)
}

#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries",
    tag = "ledger",
    request_body = NewEntryBody,
    responses(
        (status = 201, description = "The entry, with its lines", body = Entry),
        (status = 400, description = "The entry does not balance, a line names neither an account nor a category, or a tag or a place is not one", body = austeris_common::error::ErrorBody),
        (status = 404, description = "A line or the counterparty is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The same entry was recorded a moment ago", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_entry(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewEntryBody>) -> AppResult<(StatusCode, Json<Entry>)> {
    let lines = validate_lines(new.lines)?;
    let place = new.place.as_ref().map(PlaceBody::checked).transpose()?;
    let tags = tag_names(&new.tags)?;

    let posted = repository::post_entry(
        &pool,
        caller.id(),
        &caller.into(),
        &NewEntry {
            occurred_on: new.occurred_on.unwrap_or_else(today),
            description: new.description.trim().to_owned(),
            // An entry a person types has no key: they are not retrying, they
            // are recording, and two coffees on one day are two entries.
            idempotency_key: None,
            source: None,
            kept: false,
            pending: new.pending,
            counterparty: new.counterparty_id.map(CounterpartyChoice::Existing),
            place,
            tags,
            lines,
            files: Vec::new(),
            allow_duplicate: new.allow_duplicate,
        },
    )
    .await
    .map_err(refused_or_missing)?;

    Ok((StatusCode::CREATED, Json(posted.entry)))
}

/// Turns the lines a client stated into lines the repository can write.
///
/// Refuses here rather than letting the database do it, because the database's
/// message is about a constraint and this one is about what the caller sent.
/// The balance rule is still checked there, for every other writer.
fn validate_lines(lines: Vec<NewEntryLine>) -> AppResult<Vec<NewLine>> {
    if lines.len() < 2 {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "an entry has at least two sides: money leaves somewhere and arrives somewhere"
        )));
    }

    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let side = match (line.account_id, line.category_id, line.conversion) {
            (Some(account), None, false) => Side::Account(account),
            (None, Some(category), false) => Side::Category(category),
            (None, None, true) => Side::Conversion,
            _ => {
                return Err(AppError::bad_request(anyhow::anyhow!(
                    "a line names an account, a category or a conversion - exactly one of them"
                )));
            }
        };
        if line.amount.is_zero() {
            return Err(AppError::bad_request(anyhow::anyhow!("a line of zero records nothing")));
        }

        out.push(NewLine {
            side,
            amount: line.amount,
            currency: currency_code(&line.currency)?,
            note: line.note.trim().to_owned(),
        });
    }

    // Stated here as well as in the database: the caller gets "this does not
    // balance, PYG is off by 500" instead of a constraint's name.
    let mut currencies: Vec<&str> = out.iter().map(|line| line.currency.as_str()).collect();
    currencies.sort_unstable();
    currencies.dedup();
    for currency in currencies {
        let sum: Decimal = out.iter().filter(|line| line.currency == currency).map(|line| line.amount).sum();
        if !sum.is_zero() {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "the entry does not balance in {currency}: the lines sum to {sum}"
            )));
        }
    }

    Ok(out)
}

/// An entry as it was recorded, and what converting it did.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Recorded {
    /// The entry, with its lines.
    pub entry: Entry,
    /// When money changed currency on the way: both amounts, the rate they
    /// imply, the day's rate and its age, and what the difference cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversion: Option<Summary>,
    /// A counterparty the line named for the first time, created with it. Said
    /// so a mistyped name is seen the first time rather than found in a report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_counterparty: Option<Counterparty>,
    /// Tags the line used for the first time, created with it - for the same
    /// reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_tags: Vec<String>,
}

/// What exchanging money between two accounts carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ExchangeBody {
    /// The day the money changed hands. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
    /// What a person would call it: the exchange office, the bank.
    #[serde(default)]
    pub description: String,
    /// The account the money left.
    pub from_account: Uuid,
    /// How much left it, in its own currency.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "600000")]
    pub given: Decimal,
    /// The account the money arrived in.
    pub to_account: Uuid,
    /// How much arrived, in its own currency.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "100")]
    pub got: Decimal,
    /// The exchange office or the bank, when it is one of your counterparties.
    pub counterparty_id: Option<Uuid>,
    /// Where it happened.
    pub place: Option<PlaceBody>,
    /// Labels, by name.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Record it even when the same exchange was recorded a moment ago.
    #[serde(default)]
    pub allow_duplicate: bool,
}

/// Records money changed from one currency into another.
///
/// The two amounts are what changed hands, and the rate of the deal is what they
/// imply - never the day's rate. The day's rate is the reference instead: what
/// was given beyond what the received amount was worth at it is what the bank or
/// the exchange office kept, and it is recorded as a line of its own under the
/// category the ledger keeps for exchange fees. When no fresh rate is known the
/// fee cannot be measured, and `conversion.no_fee` says so.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/exchanges",
    tag = "ledger",
    request_body = ExchangeBody,
    responses(
        (status = 201, description = "The entry, and what the exchange cost beyond the day's rate", body = Recorded),
        (status = 400, description = "The amounts are not positive, or both accounts are in one currency", body = austeris_common::error::ErrorBody),
        (status = 404, description = "An account is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The same exchange was recorded a moment ago", body = austeris_common::error::ErrorBody),
    ),
)]
async fn exchange(State(pool): State<PgPool>, caller: Caller, Json(body): Json<ExchangeBody>) -> AppResult<(StatusCode, Json<Recorded>)> {
    let owner_id = caller.id();
    let from = repository::account(&pool, owner_id, body.from_account)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account to exchange from")))?;
    let to = repository::account(&pool, owner_id, body.to_account)
        .await?
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account to exchange into")))?;

    let occurred_on = body.occurred_on.unwrap_or_else(today);
    let place = body.place.as_ref().map(PlaceBody::checked).transpose()?;
    let tags = tag_names(&body.tags)?;
    let plan = payment::plan(
        &pool,
        Money {
            amount: body.given,
            currency: from.currency.clone(),
        },
        Money {
            amount: body.got,
            currency: to.currency.clone(),
        },
        occurred_on,
    )
    .await?;
    let lines = payment::conversion_lines(&pool, owner_id, &plan, Side::Account(from.id), Side::Account(to.id)).await?;
    let summary = plan.summary;

    let description = match body.description.trim() {
        "" => format!("{} -> {}", from.name, to.name),
        said => said.to_owned(),
    };
    let posted = repository::post_entry(
        &pool,
        owner_id,
        &caller.into(),
        &NewEntry {
            occurred_on,
            description,
            counterparty: body.counterparty_id.map(CounterpartyChoice::Existing),
            place,
            tags,
            lines,
            allow_duplicate: body.allow_duplicate,
            ..NewEntry::default()
        },
    )
    .await
    .map_err(refused_or_missing)?;

    Ok((
        StatusCode::CREATED,
        Json(Recorded {
            entry: posted.entry,
            conversion: Some(summary),
            new_counterparty: None,
            new_tags: Vec::new(),
        }),
    ))
}

/// Which entries to list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct EntryQuery {
    /// Only entries on or after this day.
    pub from: Option<NaiveDate>,
    /// Only entries on or before this day.
    pub to: Option<NaiveDate>,
    /// Only entries touching this account.
    pub account: Option<Uuid>,
    /// Only entries attributed to this category.
    pub category: Option<Uuid>,
    /// Only entries carrying this tag, by name in any case.
    pub tag: Option<String>,
    /// Only entries with this counterparty.
    pub counterparty: Option<Uuid>,
    /// Only entries at this place.
    pub place: Option<Uuid>,
    /// Only held entries (`pending`), or only posted ones (`cleared`).
    pub status: Option<EntryStatus>,
    /// How many to return. Fifty when omitted, two hundred at most.
    pub limit: Option<i64>,
    /// How many to skip.
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/entries",
    tag = "ledger",
    params(EntryQuery),
    responses(
        (status = 200, description = "The entries, newest first", body = Vec<Entry>),
        (status = 400, description = "The window ends before it starts", body = austeris_common::error::ErrorBody),
    ),
)]
async fn list_entries(State(pool): State<PgPool>, caller: Caller, Query(query): Query<EntryQuery>) -> AppResult<Json<Vec<Entry>>> {
    if let (Some(from), Some(to)) = (query.from, query.to)
        && from > to
    {
        return Err(AppError::bad_request(anyhow::anyhow!("the window ends before it starts")));
    }

    let filter = EntryFilter {
        from: query.from,
        to: query.to,
        account_id: query.account,
        category_id: query.category,
        tag: query.tag.as_deref().map(|tag| tag.trim().trim_start_matches('#').to_owned()),
        counterparty_id: query.counterparty,
        place_id: query.place,
        status: query.status,
        // Bounded rather than trusted: an unbounded page is a way to pull a
        // decade of entries into a Raspberry Pi's memory with one request.
        limit: query.limit.unwrap_or(50).clamp(1, 200),
        offset: query.offset.unwrap_or(0).max(0),
    };

    Ok(Json(repository::entries(&pool, caller.id(), &filter).await?))
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/entries/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    responses(
        (status = 200, description = "The entry, with its lines", body = Entry),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
    ),
)]
async fn read_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<Json<Entry>> {
    repository::entry(&pool, caller.id(), id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such entry")))
}

/// Deletes an entry and its lines, to the trash: for thirty days it can be
/// restored as it was, with its files.
///
/// A whole entry, never one line: half an entry is money from nowhere, and the
/// balance rule would refuse it anyway.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/entries/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    responses(
        (status = 204, description = "In the trash"),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
        (status = 409, description = "A module keeps the entry - a trade's money - and it is deleted there", body = austeris_common::error::ErrorBody),
    ),
)]
async fn delete_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    let deleted = repository::delete_entry(&pool, caller.id(), &caller.into(), id)
        .await
        .map_err(|error| kept_by_a_module(&error).unwrap_or_else(|| AppError::internal(error)))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found(anyhow::anyhow!("no such entry")))
    }
}

/// What balances are asked for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct BalanceQuery {
    /// As of the end of this day. Today when omitted.
    pub as_of: Option<NaiveDate>,
    /// Also convert every balance into this currency, at the rate for that day.
    pub currency: Option<String>,
}

/// What every account holds, and what that adds up to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Balances {
    /// The day these are as of.
    pub as_of: NaiveDate,
    /// One per account, in the account's own currency.
    pub accounts: Vec<Balance>,
    /// The sum, when a currency to sum in was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<Total>,
}

/// The sum of every balance that could be converted.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Total {
    /// What it is in.
    pub currency: String,
    /// The sum.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1575.00")]
    pub amount: Decimal,
    /// The sum of what is available on each account, at the same rates.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "1505.00")]
    pub available: Decimal,
    /// Currencies no rate was found for, whose balances are *not* in `amount`.
    ///
    /// Named rather than dropped: a total that silently omits a third of
    /// someone's money looks like an answer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unconverted: Vec<String>,
    /// Currencies converted at a rate older than a long weekend. Their
    /// balances are in `amount`, at a rate that may no longer be true; each
    /// balance's `rate_used` says how old.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stale: Vec<String>,
}

/// Balances are computed from the entries every time, never stored: a stored
/// balance is a second copy of what the lines already say, and the day the two
/// disagree there is no way to tell which is right.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/balances",
    tag = "ledger",
    params(BalanceQuery),
    responses((status = 200, description = "What each account holds", body = Balances)),
)]
async fn balances(State(pool): State<PgPool>, caller: Caller, Query(query): Query<BalanceQuery>) -> AppResult<Json<Balances>> {
    let as_of = query.as_of.unwrap_or_else(today);
    let mut accounts = repository::balances(&pool, caller.id(), as_of).await?;

    let total = match &query.currency {
        Some(currency) => {
            let currency = currency_code(currency)?;
            let summed = balance::convert(&pool, &mut accounts, &currency, as_of).await?;
            Some(Total {
                currency: summed.currency,
                amount: summed.amount,
                available: summed.available,
                unconverted: summed.unconverted,
                stale: summed.stale,
            })
        }
        None => None,
    };

    Ok(Json(Balances { as_of, accounts, total }))
}

/// What recording a rate carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewRate {
    /// The currency being priced.
    pub base: String,
    /// The currency it is priced in.
    pub quote: String,
    /// The day it applies to. Today when omitted.
    pub on_date: Option<NaiveDate>,
    /// How many of `quote` one `base` buys.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.00013")]
    pub rate: Decimal,
}

/// Which rates to list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct RateQuery {
    /// The currency being priced.
    pub base: String,
    /// The currency it is priced in.
    pub quote: String,
    /// How many to return, newest first. Thirty when omitted.
    pub limit: Option<i64>,
}

/// Records what one currency was worth in another on a day.
///
/// Kept here rather than read from `market` on every conversion: a rate used to
/// value a past entry must not change when a source revises its history.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/rates",
    tag = "ledger",
    request_body = NewRate,
    responses(
        (status = 204, description = "Recorded"),
        (status = 400, description = "A rate is positive, and its currencies differ", body = austeris_common::error::ErrorBody),
    ),
)]
async fn record_rate(State(pool): State<PgPool>, _caller: Caller, Json(new): Json<NewRate>) -> AppResult<StatusCode> {
    let (base, quote) = (currency_code(&new.base)?, currency_code(&new.quote)?);
    if base == quote {
        return Err(AppError::bad_request(anyhow::anyhow!("a currency is worth one of itself")));
    }
    if !new.rate.is_sign_positive() || new.rate.is_zero() {
        return Err(AppError::bad_request(anyhow::anyhow!("a rate is a positive number")));
    }

    repository::record_rate(&pool, &base, &quote, new.on_date.unwrap_or_else(today), new.rate, "manual").await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/ledger/rates",
    tag = "ledger",
    params(RateQuery),
    responses((status = 200, description = "The rates recorded for the pair, newest first", body = Vec<ExchangeRate>)),
)]
async fn list_rates(State(pool): State<PgPool>, _caller: Caller, Query(query): Query<RateQuery>) -> AppResult<Json<Vec<ExchangeRate>>> {
    let (base, quote) = (currency_code(&query.base)?, currency_code(&query.quote)?);
    Ok(Json(repository::rates(&pool, &base, &quote, query.limit.unwrap_or(30).clamp(1, 365)).await?))
}

/// Which rate is asked for.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct RateAtQuery {
    /// The currency being priced.
    pub base: String,
    /// The currency it is priced in.
    pub quote: String,
    /// The day. Today when omitted.
    pub on: Option<NaiveDate>,
}

/// The rate in force on a day: what a screen suggests before an amount in
/// another currency is recorded.
///
/// The newest rate not after the day, for the pair as recorded, inverted, or
/// through a third currency - with the day it was set for, its age by the day
/// asked about, and whether that makes it stale. A stale rate is still answered:
/// it is the last thing known, and the flag is what keeps it from being read as
/// today's.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/rates/at",
    tag = "ledger",
    params(RateAtQuery),
    responses(
        (status = 200, description = "The rate in force, and how old it is", body = RateInForce),
        (status = 404, description = "No rate for the pair is known on or before the day", body = austeris_common::error::ErrorBody),
    ),
)]
async fn rate_at(State(pool): State<PgPool>, _caller: Caller, Query(query): Query<RateAtQuery>) -> AppResult<Json<RateInForce>> {
    let (base, quote) = (currency_code(&query.base)?, currency_code(&query.quote)?);
    let on = query.on.unwrap_or_else(today);
    repository::rate_in_force(&pool, &base, &quote, on)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no {base}->{quote} rate is known on or before {on}")))
}

/// Today, as the ledger reckons it.
///
/// The server's day. A person entering a receipt at one in the morning in
/// another timezone would want their own, which is what the `occurred_on` field
/// is for - the default is only a default.
fn today() -> NaiveDate {
    Utc::now().date_naive()
}

/// A currency code, normalised.
///
/// Three letters, upper-cased, so `usd` and `USD` are one currency rather than
/// two that never balance against each other.
fn currency_code(raw: &str) -> AppResult<String> {
    crate::currency::code(raw).ok_or_else(|| {
        AppError::bad_request(anyhow::anyhow!(
            "`{raw}` is not a currency code; three letters, as in USD or PYG, or an exchange's ticker of up to five, as in USDT"
        ))
    })
}

/// A unique-violation is the caller's conflict; anything else is a fault.
fn conflict_or(error: anyhow::Error, message: &'static str) -> AppError {
    if is_unique_violation(&error) {
        AppError::new(StatusCode::CONFLICT, anyhow::anyhow!(message))
    } else {
        AppError::internal(error)
    }
}

/// Whether an error is PostgreSQL refusing a duplicate.
fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .any(|error| error.as_database_error().is_some_and(|db| db.code().as_deref() == Some("23505")))
}

/// Turns a failed post into the answer the caller can act on.
///
/// The database's rules - the entry balances, a conversion converts, an
/// account line is in the account's currency - and the ownership check all
/// come back as errors from the repository. They are the caller's to fix, and
/// a 500 would tell them to contact an administrator about their own
/// arithmetic. A rule is recognised by its SQLSTATE rather than its wording, so
/// a rule added later is a 400 without anyone remembering to list it here.
fn refused_or_missing(error: anyhow::Error) -> AppError {
    if let Some(twin) = error.downcast_ref::<PossibleDuplicate>() {
        return AppError::new(StatusCode::CONFLICT, anyhow::anyhow!("{twin}")).with_code(
            Code::PossibleDuplicate,
            serde_json::json!({ "entry_id": twin.entry_id, "seconds": twin.seconds }),
        );
    }
    if let Some(kept) = kept_by_a_module(&error) {
        return kept;
    }
    let refused = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .find(|db| db.code().as_deref() == Some(CHECK_VIOLATION))
        .map(|db| db.message().to_owned());
    if let Some(message) = refused {
        return AppError::bad_request(anyhow::anyhow!("{message}"));
    }
    if is_foreign_key_violation(&error) {
        return AppError::not_found(anyhow::anyhow!("the entry names a counterparty or a category that is not yours"));
    }

    let text = format!("{error:#}");
    if text.contains("does not exist") {
        AppError::not_found(anyhow::anyhow!("{text}"))
    } else {
        AppError::internal(error)
    }
}

/// The answer to changing an entry a module keeps, when that is what was
/// refused: the repository says which module, and the database - for a path
/// that did not ask first - says it in its error's detail.
pub(crate) fn kept_by_a_module(error: &anyhow::Error) -> Option<AppError> {
    let module = error.downcast_ref::<KeptEntry>().map(|kept| kept.module.clone()).or_else(|| {
        error
            .chain()
            .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
            .filter_map(|error| error.as_database_error())
            .filter(|db| db.code().as_deref() == Some(KEPT_BY_A_MODULE))
            .find_map(|db| {
                db.try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                    .and_then(|pg| pg.detail().map(ToOwned::to_owned))
            })
    })?;
    Some(
        AppError::new(
            StatusCode::CONFLICT,
            anyhow::anyhow!("this entry is kept by {module}: its day, description and money change there, and it is deleted there"),
        )
        .with_code(Code::EntryKept, serde_json::json!({ "module": module })),
    )
}

/// The code the ledger's schema refuses a change to a kept entry with.
const KEPT_BY_A_MODULE: &str = "AUK01";

/// Whether an error is PostgreSQL refusing a reference to a row that is not
/// there - or, the keys naming the owner, that is someone else's.
fn is_foreign_key_violation(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .any(|error| error.as_database_error().is_some_and(|db| db.code().as_deref() == Some(FOREIGN_KEY_VIOLATION)))
}

/// PostgreSQL's code for a rule of the schema refusing a write.
const CHECK_VIOLATION: &str = "23514";

/// PostgreSQL's code for a reference to a row that is not there.
const FOREIGN_KEY_VIOLATION: &str = "23503";

/// Whether an error is PostgreSQL refusing to delete a row something still
/// points at.
///
/// Two codes, because which one comes depends on how the reference was
/// declared: `ON DELETE RESTRICT` answers `23001`, the default `NO ACTION`
/// answers `23503`. Matching one of them only is a delete that fails as a 500
/// the day a migration changes the other.
fn is_still_referenced(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .any(|db| matches!(db.code().as_deref(), Some(RESTRICT_VIOLATION | FOREIGN_KEY_VIOLATION)))
}

/// PostgreSQL's code for a delete refused by `ON DELETE RESTRICT`.
const RESTRICT_VIOLATION: &str = "23001";

/// This service's share of the platform's `OpenAPI` document.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_accounts,
        read_account,
        create_account,
        close_account,
        list_categories,
        create_category,
        list_entries,
        read_entry,
        create_entry,
        quick::quick_entry,
        quick::preview,
        delete_entry,
        holds::change_entry,
        holds::clear_entry,
        templates::repeat,
        history::history,
        attachments::attach,
        attachments::content,
        attachments::remove,
        history::trash,
        history::empty,
        history::restore_entry,
        history::erase_entry,
        history::restore_attachment,
        history::erase_attachment,
        templates::list_templates,
        templates::create_template,
        templates::change_template,
        templates::order_templates,
        templates::delete_template,
        templates::use_template,
        exchange,
        balances,
        details::totals,
        details::list_counterparties,
        details::read_counterparty,
        details::create_counterparty,
        details::change_counterparty,
        details::delete_counterparty,
        details::list_places,
        details::change_place,
        details::list_tags,
        details::rename_tag,
        details::delete_tag,
        record_rate,
        list_rates,
        rate_at,
    ),
    components(schemas(
        Account,
        AccountKind,
        Category,
        Purpose,
        Flow,
        Entry,
        EntryStatus,
        CounterpartyRef,
        Counterparty,
        CounterpartyKind,
        Place,
        Tag,
        Line,
        LineSide,
        Attachment,
        Revision,
        RevisionAction,
        EntryField,
        Who,
        WhoKind,
        Trash,
        TrashedEntry,
        TrashedAttachment,
        Balance,
        Balances,
        Total,
        ExchangeRate,
        RateInForce,
        Recorded,
        ExchangeBody,
        Money,
        Summary,
        exchange::DealRate,
        exchange::NoFee,
        NewAccount,
        Closing,
        NewCategory,
        NewEntryBody,
        NewEntryLine,
        PlaceBody,
        quick::QuickEntry,
        quick::Preview,
        quick::Named,
        Template,
        templates::NewTemplate,
        templates::TemplatePatch,
        templates::TemplateOrder,
        templates::UseTemplate,
        templates::Repeat,
        NewRate,
        holds::EntryPatch,
        holds::ClearBody,
        details::NewCounterparty,
        details::CounterpartyPatch,
        details::TagRename,
        details::Totals,
        GroupBy,
        Group,
        Flowed,
    )),
    tags((name = "ledger", description = "Accounts, categories, the movements between them, who, where and which labels each one was, the files kept with them, what happened to each, and the trash")),
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::{NewEntryLine, currency_code, validate_lines};
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use uuid::Uuid;

    fn line(amount: &str, currency: &str) -> NewEntryLine {
        NewEntryLine {
            account_id: Some(Uuid::new_v4()),
            category_id: None,
            conversion: false,
            amount: Decimal::from_str(amount).expect("a decimal"),
            currency: currency.to_owned(),
            note: String::new(),
        }
    }

    #[test]
    fn a_currency_is_three_to_five_letters_in_one_case() {
        assert_eq!(currency_code("usd").expect("a code"), "USD");
        assert_eq!(currency_code("  PYG ").expect("a code"), "PYG");
        // An exchange's cash: what a trade there is paid in.
        assert_eq!(currency_code("usdt").expect("a code"), "USDT");
        assert_eq!(currency_code("FDUSD").expect("a code"), "FDUSD");
        // Two currencies that differ only in case would never balance against
        // each other, and the entry would be refused for arithmetic that is
        // actually correct.
        assert_eq!(currency_code("usd").expect("a code"), currency_code("USD").expect("a code"));

        for bad in ["", "US", "USDTTT", "US1", "US-T", "долл"] {
            assert!(currency_code(bad).is_err(), "`{bad}` was accepted as a currency");
        }
    }

    #[test]
    fn an_entry_that_does_not_balance_says_which_currency_and_by_how_much() {
        let error = validate_lines(vec![line("-45000", "PYG"), line("44000", "PYG")]).expect_err("should be refused");
        let message = error.to_string();
        assert!(message.contains("PYG"), "{message}");
        assert!(message.contains("-1000"), "{message}");
    }

    #[test]
    fn a_balanced_entry_passes() {
        assert!(validate_lines(vec![line("-45000", "PYG"), line("45000", "PYG")]).is_ok());
    }

    #[test]
    fn each_currency_has_to_balance_on_its_own() {
        // The two currencies of an exchange sum to zero together only by
        // coincidence of magnitude; each has to hold on its own, or a 10 USD
        // purchase could be "balanced" by 10 PYG.
        let mixed = validate_lines(vec![line("-10", "USD"), line("10", "PYG")]);
        assert!(mixed.is_err(), "an entry balanced across currencies was accepted");

        let exchange = validate_lines(vec![line("-75000", "PYG"), line("75000", "PYG"), line("-10", "USD"), line("10", "USD")]);
        assert!(exchange.is_ok(), "a real exchange was refused");
    }

    #[test]
    fn a_line_names_one_side_and_only_one() {
        let neither = NewEntryLine {
            account_id: None,
            category_id: None,
            ..line("100", "USD")
        };
        assert!(
            validate_lines(vec![neither, line("-100", "USD")]).is_err(),
            "a line naming nothing was accepted"
        );

        let both = NewEntryLine {
            category_id: Some(Uuid::new_v4()),
            ..line("100", "USD")
        };
        assert!(validate_lines(vec![both, line("-100", "USD")]).is_err(), "a line naming both was accepted");

        // A conversion names no account: one that did would be two sides at
        // once, and which balance it moved would depend on who read it.
        let conversion_on_account = NewEntryLine {
            conversion: true,
            ..line("100", "USD")
        };
        assert!(
            validate_lines(vec![conversion_on_account, line("-100", "USD")]).is_err(),
            "a conversion naming an account was accepted"
        );
        let conversion = NewEntryLine {
            account_id: None,
            conversion: true,
            ..line("100", "USD")
        };
        assert!(
            validate_lines(vec![conversion, line("-100", "USD")]).is_ok(),
            "a plain conversion line was refused"
        );
    }

    #[test]
    fn one_line_is_not_an_entry() {
        // Money that leaves somewhere and arrives nowhere.
        assert!(validate_lines(vec![line("-45000", "PYG")]).is_err());
        assert!(validate_lines(Vec::new()).is_err());
    }

    #[test]
    fn a_line_of_zero_is_refused_before_it_reaches_the_database() {
        assert!(validate_lines(vec![line("0", "PYG"), line("0", "PYG")]).is_err());
    }
}
