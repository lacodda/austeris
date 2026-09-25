//! The ledger service's HTTP surface.
//!
//! Private to the compose network; the gateway forwards `/api/v1/ledger/...`
//! here (ADR 0001). Every handler is scoped by [`Caller`] - the person the
//! gateway vouched for - and there is no path that reads across people.

use austeris_common::{AppError, AppResult, Caller, health};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::exchange::{self, Money, Summary};
use crate::model::{Account, AccountKind, Balance, Category, Entry, ExchangeRate, Flow, Line, LineSide, Purpose, RateInForce};
use crate::repository::{CategoryMatch, EntryFilter, NewEntry, NewLine, Side};
use crate::{MIGRATOR, balance, currency, parse, repository};

/// Builds the service's router.
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .merge(health::routes(Some(health::Readiness::new(pool.clone(), &MIGRATOR))))
        .route("/ledger/accounts", get(list_accounts).post(create_account))
        .route("/ledger/accounts/{id}", get(read_account))
        .route("/ledger/accounts/{id}/close", post(close_account))
        .route("/ledger/categories", get(list_categories).post(create_category))
        .route("/ledger/entries", get(list_entries).post(create_entry))
        .route("/ledger/entries/quick", post(quick_entry))
        .route("/ledger/entries/{id}", get(read_entry).delete(delete_entry))
        .route("/ledger/exchanges", post(exchange))
        .route("/ledger/balances", get(balances))
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
    /// The sides of the movement. At least two, summing to zero in every
    /// currency they touch.
    pub lines: Vec<NewEntryLine>,
}

#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries",
    tag = "ledger",
    request_body = NewEntryBody,
    responses(
        (status = 201, description = "The entry, with its lines", body = Entry),
        (status = 400, description = "The entry does not balance, or a line names neither an account nor a category", body = austeris_common::error::ErrorBody),
        (status = 404, description = "A line names something that is not yours", body = austeris_common::error::ErrorBody),
    ),
)]
async fn create_entry(State(pool): State<PgPool>, caller: Caller, Json(new): Json<NewEntryBody>) -> AppResult<(StatusCode, Json<Entry>)> {
    let lines = validate_lines(new.lines)?;

    let posted = repository::post_entry(
        &pool,
        caller.id(),
        &NewEntry {
            occurred_on: new.occurred_on.unwrap_or_else(today),
            description: new.description.trim().to_owned(),
            // An entry a person types has no key: they are not retrying, they
            // are recording, and two coffees on one day are two entries.
            idempotency_key: None,
            source: None,
            lines,
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

/// What a one-line entry carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct QuickEntry {
    /// The line, as a person would type it: `45000 food lunch`.
    pub text: String,
    /// The day the money moved. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
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
}

/// Records an entry from one typed line.
///
/// The same parser the CLI uses (`austeris add`), so a sentence means the same
/// thing wherever it is typed. What the parser cannot know - which account,
/// which category - is resolved here against this person's own.
///
/// An amount in a currency other than the account's is converted: at the rate
/// the line states with `@`, or at the day's rate when it states none. The
/// category keeps the amount as it was charged, the account gets it in its own
/// currency, and `conversion` says which rate was used and how old it was.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/quick",
    tag = "ledger",
    request_body = QuickEntry,
    responses(
        (status = 201, description = "The entry, with its lines, and the conversion when there was one", body = Recorded),
        (status = 400, description = "The line could not be read, or it is in a currency no rate is known for", body = austeris_common::error::ErrorBody),
        (status = 404, description = "It names a category or account you do not have", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The name means more than one category", body = austeris_common::error::ErrorBody),
    ),
)]
async fn quick_entry(State(pool): State<PgPool>, caller: Caller, Json(quick): Json<QuickEntry>) -> AppResult<(StatusCode, Json<Recorded>)> {
    let spoken = parse::line(&quick.text).map_err(AppError::bad_request)?;
    let (new, conversion) = resolve(&pool, caller.id(), &spoken, quick.occurred_on.unwrap_or_else(today)).await?;
    let posted = repository::post_entry(&pool, caller.id(), &new).await.map_err(refused_or_missing)?;
    Ok((
        StatusCode::CREATED,
        Json(Recorded {
            entry: posted.entry,
            conversion,
        }),
    ))
}

/// Turns what a person said into an entry against their own accounts.
///
/// Its own function rather than part of the handler: the CLI resolves the same
/// sentence through the API, and the phone screen will, so the rules for what
/// `food` and `cash` mean live in one place. Two lines for a plain expense -
/// the account and the category - and a conversion between them when the
/// amount was in another currency.
async fn resolve(pool: &PgPool, owner_id: Uuid, spoken: &parse::Spoken, occurred_on: NaiveDate) -> AppResult<(NewEntry, Option<Summary>)> {
    // `45000 pen office`: Peruvian soles on `office`, or 45 000 on `pen`? The
    // person's own categories decide - a word they chose outranks a list of
    // the world's currencies they may never use.
    let is_their_category = match (&spoken.currency, &spoken.if_not_a_currency) {
        (Some(code), Some(_)) => !matches!(repository::category_by_name(pool, owner_id, code).await?, CategoryMatch::None),
        _ => false,
    };
    let spoken = match &spoken.if_not_a_currency {
        Some(other) if is_their_category => other.as_ref(),
        _ => spoken,
    };

    let category = match repository::category_by_name(pool, owner_id, &spoken.category).await? {
        CategoryMatch::One(category) => *category,
        CategoryMatch::None => {
            return Err(AppError::not_found(anyhow::anyhow!(
                "you have no category called `{}`; create it first",
                spoken.category
            )));
        }
        // Resolved by asking, not by guessing: filing money under the wrong
        // one of two categories with the same name is a mistake nobody sees
        // until a report is wrong.
        CategoryMatch::Several(names) => {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                anyhow::anyhow!(
                    "`{}` means more than one category ({}); say which, as in `living/{}`",
                    spoken.category,
                    names.join(", "),
                    spoken.category
                ),
            ));
        }
    };

    let account = match &spoken.account {
        Some(name) => repository::account_by_name(pool, owner_id, name)
            .await?
            .ok_or_else(|| AppError::not_found(anyhow::anyhow!("you have no account called `{name}`")))?,
        None => default_account(pool, owner_id).await?,
    };

    // A line saying "spent" filed under an income category (or the reverse) is
    // a person having typed the wrong word, and it would quietly make every
    // income report wrong.
    let expected = match spoken.flow {
        parse::Flow::Expense => Flow::Expense,
        parse::Flow::Income => Flow::Income,
    };
    if category.flow != expected {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "`{}` is a category of {}; write `+{}` for money coming in",
            category.name,
            match category.flow {
                Flow::Income => "money coming in",
                Flow::Expense => "money going out",
            },
            spoken.amount
        )));
    }

    // A rate only means something for an amount in another currency; stated
    // on a plain line it would be silently ignored, and the person would think
    // it had been applied.
    let foreign = spoken.currency.as_deref().filter(|named| !named.eq_ignore_ascii_case(&account.currency));
    if foreign.is_none() && spoken.rate.is_some() {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "a rate converts an amount in another currency; this one is already in {}",
            account.currency
        )));
    }

    let (lines, conversion) = match foreign {
        None => {
            // The signs: money spent leaves the account and lands on the
            // expense category, money earned arrives in the account from the
            // income category.
            let (on_account, on_category) = match spoken.flow {
                parse::Flow::Expense => (-spoken.amount, spoken.amount),
                parse::Flow::Income => (spoken.amount, -spoken.amount),
            };
            let lines = vec![
                NewLine {
                    side: Side::Account(account.id),
                    amount: on_account,
                    currency: account.currency.clone(),
                    note: String::new(),
                },
                NewLine {
                    side: Side::Category(category.id),
                    amount: on_category,
                    currency: account.currency.clone(),
                    note: spoken.note.clone(),
                },
            ];
            (lines, None)
        }
        Some(foreign) => {
            let (lines, summary) = foreign_amount(pool, owner_id, spoken, &account, &category, foreign, occurred_on).await?;
            (lines, Some(summary))
        }
    };

    Ok((
        NewEntry {
            occurred_on,
            description: spoken.note.clone(),
            idempotency_key: None,
            source: None,
            lines,
        },
        conversion,
    ))
}

/// The lines of an amount charged or paid in a currency the account is not in.
///
/// The category keeps the amount as it was charged - a 10 USD subscription is
/// 10 USD in every report kept in dollars - and the account gets what it
/// actually moved by, in its own currency, rounded to the places that currency
/// has. The conversion between them is where each currency balances.
async fn foreign_amount(
    pool: &PgPool,
    owner_id: Uuid,
    spoken: &parse::Spoken,
    account: &Account,
    category: &Category,
    foreign: &str,
    occurred_on: NaiveDate,
) -> AppResult<(Vec<NewLine>, Summary)> {
    // How many of the account's currency one unit of the foreign one cost: as
    // the line said, or the day's rate.
    let rate = match spoken.rate {
        Some(stated) => stated,
        None => {
            repository::rate_in_force(pool, foreign, &account.currency, occurred_on)
                .await?
                .ok_or_else(|| {
                    AppError::bad_request(anyhow::anyhow!(
                        "no {foreign}->{} rate is known on or before {occurred_on}; state the one you were charged, as in `@5965`",
                        account.currency
                    ))
                })?
                .rate
        }
    };
    let in_account = currency::round(spoken.amount * rate, &account.currency);
    if in_account.is_zero() {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "{} {foreign} is worth nothing in {}",
            spoken.amount,
            account.currency
        )));
    }

    // Spending gives the account's currency for the foreign one; earning gives
    // the foreign one for the account's.
    let (given, got, from, to) = match spoken.flow {
        parse::Flow::Expense => (
            Money {
                amount: in_account,
                currency: account.currency.clone(),
            },
            Money {
                amount: spoken.amount,
                currency: foreign.to_owned(),
            },
            Side::Account(account.id),
            Side::Category(category.id),
        ),
        parse::Flow::Income => (
            Money {
                amount: spoken.amount,
                currency: foreign.to_owned(),
            },
            Money {
                amount: in_account,
                currency: account.currency.clone(),
            },
            Side::Category(category.id),
            Side::Account(account.id),
        ),
    };

    let (mut lines, summary) = convert(pool, owner_id, given, got, from, to, occurred_on).await?;
    for line in &mut lines {
        if line.side == Side::Category(category.id) {
            line.note.clone_from(&spoken.note);
        }
    }
    Ok((lines, summary))
}

/// Plans a conversion against the day's rate and builds its lines, creating
/// the fee category the first time a fee needs one.
async fn convert(pool: &PgPool, owner_id: Uuid, given: Money, got: Money, from: Side, to: Side, occurred_on: NaiveDate) -> AppResult<(Vec<NewLine>, Summary)> {
    // The reference prices what was got in what was given: how many of the
    // given currency the received amount was worth that day.
    let reference = repository::rate_in_force(pool, &got.currency, &given.currency, occurred_on).await?;
    let plan = exchange::plan(given, got, reference).map_err(AppError::bad_request)?;

    let fee_category = if plan.has_fee() {
        Some(
            repository::purpose_category(pool, owner_id, Purpose::ExchangeFees)
                .await
                .map_err(AppError::bad_request)?
                .id,
        )
    } else {
        None
    };

    let lines = plan.lines(from, to, fee_category).map_err(AppError::internal)?;
    Ok((lines, plan.summary))
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
    let (lines, summary) = convert(
        &pool,
        owner_id,
        Money {
            amount: body.given,
            currency: from.currency.clone(),
        },
        Money {
            amount: body.got,
            currency: to.currency.clone(),
        },
        Side::Account(from.id),
        Side::Account(to.id),
        occurred_on,
    )
    .await?;

    let description = match body.description.trim() {
        "" => format!("{} -> {}", from.name, to.name),
        said => said.to_owned(),
    };
    let posted = repository::post_entry(
        &pool,
        owner_id,
        &NewEntry {
            occurred_on,
            description,
            idempotency_key: None,
            source: None,
            lines,
        },
    )
    .await
    .map_err(refused_or_missing)?;

    Ok((
        StatusCode::CREATED,
        Json(Recorded {
            entry: posted.entry,
            conversion: Some(summary),
        }),
    ))
}

/// The account a line that names none is against.
///
/// The person's only open account. Deliberately not "the first one": with two
/// open accounts there is no default anyone agreed on, and picking one would
/// post money to the wrong place every time the person forgot to say.
async fn default_account(pool: &PgPool, owner_id: Uuid) -> AppResult<Account> {
    let open: Vec<Account> = repository::accounts(pool, owner_id)
        .await?
        .into_iter()
        .filter(|account| account.closed_at.is_none())
        .collect();

    match <[Account; 1]>::try_from(open) {
        Ok([only]) => Ok(only),
        Err(rest) if rest.is_empty() => Err(AppError::not_found(anyhow::anyhow!(
            "you have no open account to record this against; create one first"
        ))),
        Err(rest) => Err(AppError::bad_request(anyhow::anyhow!(
            "say which account, as in `from {}` - you have {}",
            rest[0].name,
            rest.iter().map(|account| account.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
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

/// Deletes an entry and its lines.
///
/// A whole entry, never one line: half an entry is money from nowhere, and the
/// balance rule would refuse it anyway.
#[utoipa::path(
    delete,
    path = "/api/v1/ledger/entries/{id}",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The entry")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "No such entry", body = austeris_common::error::ErrorBody),
    ),
)]
async fn delete_entry(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>) -> AppResult<StatusCode> {
    if repository::delete_entry(&pool, caller.id(), id).await? {
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
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::bad_request(anyhow::anyhow!(
            "`{raw}` is not a currency code; three letters, as in USD or PYG"
        )));
    }
    Ok(trimmed.to_uppercase())
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
    let refused = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .find(|db| db.code().as_deref() == Some(CHECK_VIOLATION))
        .map(|db| db.message().to_owned());
    if let Some(message) = refused {
        return AppError::bad_request(anyhow::anyhow!("{message}"));
    }

    let text = format!("{error:#}");
    if text.contains("does not exist") {
        AppError::not_found(anyhow::anyhow!("{text}"))
    } else {
        AppError::internal(error)
    }
}

/// PostgreSQL's code for a rule of the schema refusing a write.
const CHECK_VIOLATION: &str = "23514";

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
        quick_entry,
        delete_entry,
        exchange,
        balances,
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
        Line,
        LineSide,
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
        QuickEntry,
        NewRate,
    )),
    tags((name = "ledger", description = "Accounts, categories and the movements between them")),
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
    fn a_currency_is_three_letters_in_one_case() {
        assert_eq!(currency_code("usd").expect("a code"), "USD");
        assert_eq!(currency_code("  PYG ").expect("a code"), "PYG");
        // Two currencies that differ only in case would never balance against
        // each other, and the entry would be refused for arithmetic that is
        // actually correct.
        assert_eq!(currency_code("usd").expect("a code"), currency_code("USD").expect("a code"));

        for bad in ["", "US", "USDD", "US1", "долл"] {
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
