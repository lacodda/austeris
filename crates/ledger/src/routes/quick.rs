//! An entry in one typed line, and what a line would record before it is.

use austeris_common::{AppError, AppResult, Caller, Code};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::payment::{Payment, Priced};
use super::{PlaceBody, Recorded, refused_or_missing, today};
use crate::exchange::{Money, Summary};
use crate::model::{Account, Category, Counterparty, Flow};
use crate::parse;
use crate::repository::{self, CategoryMatch, CounterpartyChoice};

/// What a one-line entry carries.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct QuickEntry {
    /// The line, as a person would type it: `45000 food lunch`.
    pub text: String,
    /// The day the money moved. Today when omitted.
    pub occurred_on: Option<NaiveDate>,
    /// Where it happened. Beside the line rather than in it, like the day: it
    /// is where the person is, not something they say about each purchase.
    pub place: Option<PlaceBody>,
    /// The account, when the line names none - a screen where the account is
    /// already chosen. Beside the line for the same reason as the day; a line
    /// that says `from cash` is still taken at its word.
    pub account_id: Option<Uuid>,
    /// Record it even when the same entry was recorded a moment ago.
    #[serde(default)]
    pub allow_duplicate: bool,
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
///
/// The same entry recorded within the last minute - a second press, a retried
/// request - is refused with `409` and the code `entry.possible_duplicate`;
/// `allow_duplicate` records it anyway.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/quick",
    tag = "ledger",
    request_body = QuickEntry,
    responses(
        (status = 201, description = "The entry, with its lines, and the conversion when there was one", body = Recorded),
        (status = 400, description = "The line could not be read, or it is in a currency no rate is known for", body = austeris_common::error::ErrorBody),
        (status = 404, description = "It names a category or account you do not have", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The name means more than one category, or the same entry was recorded a moment ago", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn quick_entry(State(pool): State<PgPool>, caller: Caller, Json(quick): Json<QuickEntry>) -> AppResult<(StatusCode, Json<Recorded>)> {
    let owner_id = caller.id();
    let understood = understand(&pool, owner_id, &quick).await?;
    let new_tags = understood.new_tags;
    let (entry, conversion) = understood.priced.entry(&pool, owner_id, quick.allow_duplicate).await?;
    let posted = repository::post_entry(&pool, owner_id, &entry).await.map_err(refused_or_missing)?;

    // Read back as stored, so what is reported as new is the row itself.
    let new_counterparty = match (&entry.counterparty, &posted.entry.counterparty) {
        (Some(CounterpartyChoice::New { .. }), Some(stored)) => repository::counterparty(&pool, owner_id, stored.id).await?,
        _ => None,
    };
    Ok((
        StatusCode::CREATED,
        Json(Recorded {
            entry: posted.entry,
            conversion,
            new_counterparty,
            new_tags,
        }),
    ))
}

/// What a typed line would record, worked out and not written.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Preview {
    /// The day it would be recorded on.
    pub occurred_on: NaiveDate,
    /// Which way the money would go.
    pub flow: Flow,
    /// The account it would leave or arrive in.
    pub account: Account,
    /// What it would be filed under.
    pub category: Category,
    /// The amount as the line said it, in the currency it said.
    pub amount: Money,
    /// What the account would move by, signed, in its own currency.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "-45000")]
    pub moves: rust_decimal::Decimal,
    /// What a person would call it.
    pub description: String,
    /// Who it would be with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<Named>,
    /// The labels it would carry.
    pub tags: Vec<String>,
    /// The labels it would use for the first time.
    pub new_tags: Vec<String>,
    /// Held rather than posted.
    pub pending: bool,
    /// The conversion, when money would change currency on the way - at the
    /// rate it would be recorded at, measured against the day's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversion: Option<Summary>,
}

/// A counterparty a line names: one of the person's, or a name used for the
/// first time.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Named {
    /// The counterparty, when the person has it; absent for a name recording
    /// the line would create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    /// Its name, as stored or as typed.
    pub name: String,
}

/// What a typed line would record, before it is recorded.
///
/// The same reading as recording it - the same parser, the same account,
/// category and counterparty, the same conversion at the same rate - with
/// nothing written: not the entry, not a counterparty or a tag the line names
/// for the first time, not the category a fee would go under. A screen asks
/// this while the line is being typed, so the person sees what it means
/// before they press anything, and why it would be refused.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/quick/preview",
    tag = "ledger",
    request_body = QuickEntry,
    responses(
        (status = 200, description = "What the line would record", body = Preview),
        (status = 400, description = "The line could not be read, or it is in a currency no rate is known for", body = austeris_common::error::ErrorBody),
        (status = 404, description = "It names a category or account you do not have", body = austeris_common::error::ErrorBody),
        (status = 409, description = "The name means more than one category", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn preview(State(pool): State<PgPool>, caller: Caller, Json(quick): Json<QuickEntry>) -> AppResult<Json<Preview>> {
    let understood = understand(&pool, caller.id(), &quick).await?;
    let conversion = understood.priced.conversion().cloned();
    let Priced { payment, moves, .. } = understood.priced;
    Ok(Json(Preview {
        occurred_on: payment.occurred_on,
        flow: payment.flow,
        account: payment.account,
        category: payment.category,
        amount: Money {
            amount: payment.amount,
            currency: payment.currency,
        },
        moves,
        description: payment.description,
        counterparty: understood.named.map(|named| match named {
            Spoken::Known(known) => Named {
                id: Some(known.id),
                name: known.name,
            },
            Spoken::Unknown(name) => Named { id: None, name },
        }),
        tags: payment.tags,
        new_tags: understood.new_tags,
        pending: payment.pending,
        conversion,
    }))
}

/// A typed line, read and worked out against the person's own books.
struct Understood {
    /// The payment it says, priced.
    priced: Priced,
    /// The counterparty it named.
    named: Option<Spoken>,
    /// The tags it uses for the first time.
    new_tags: Vec<String>,
}

/// Reads a line and works out what it would record, writing nothing.
async fn understand(pool: &PgPool, owner_id: Uuid, quick: &QuickEntry) -> AppResult<Understood> {
    let spoken = parse::line(&quick.text).map_err(unreadable)?;
    let occurred_on = quick.occurred_on.unwrap_or_else(today);
    let (mut payment, named) = resolve(pool, owner_id, &spoken, occurred_on, quick.account_id).await?;
    payment.place = quick.place.as_ref().map(PlaceBody::checked).transpose()?;
    let new_tags = unknown_tags(pool, owner_id, &payment.tags).await?;
    let priced = payment.priced(pool).await?;
    Ok(Understood { priced, named, new_tags })
}

/// A line that could not be read, with the code a screen says it by.
fn unreadable(error: parse::ParseError) -> AppError {
    use parse::ParseError as Why;
    let (code, params) = match &error {
        Why::Empty => (Code::LineEmpty, json!({})),
        Why::NoAmount(word) => (Code::LineNoAmount, json!({ "word": word })),
        Why::ZeroAmount => (Code::LineZeroAmount, json!({})),
        Why::NoCategory(amount) => (Code::LineNoCategory, json!({ "amount": amount })),
        Why::DanglingAccount(word) => (Code::LineDanglingAccount, json!({ "word": word })),
        Why::BadRate(word) => (Code::LineBadRate, json!({ "word": word })),
        Why::BareAt => (Code::LineBareAt, json!({})),
        Why::Twice(word) => (Code::LineTwice, json!({ "word": word })),
    };
    AppError::bad_request(error).with_code(code, params)
}

/// The counterparty a typed line named, as the person's books know it.
enum Spoken {
    /// One they have.
    Known(Counterparty),
    /// A name they have never used, as it was typed.
    Unknown(String),
}

/// Turns what a person said into a payment against their own accounts.
///
/// Its own function rather than part of the handler: the CLI resolves the same
/// sentence through the API, and so does the phone screen, so the rules for
/// what `food` and `cash` mean live in one place.
async fn resolve(
    pool: &PgPool,
    owner_id: Uuid,
    spoken: &parse::Spoken,
    occurred_on: NaiveDate,
    account_id: Option<Uuid>,
) -> AppResult<(Payment, Option<Spoken>)> {
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

    let counterparty = match &spoken.counterparty {
        Some(typed) => Some(match repository::counterparty_by_name(pool, owner_id, typed).await? {
            Some(found) => Spoken::Known(found),
            None => Spoken::Unknown(typed.clone()),
        }),
        None => None,
    };

    let category = category_for(pool, owner_id, spoken, counterparty.as_ref()).await?;

    let account = match (&spoken.account, account_id) {
        (Some(name), _) => repository::account_by_name(pool, owner_id, name).await?.ok_or_else(|| {
            AppError::not_found(anyhow::anyhow!("you have no account called `{name}`")).with_code(Code::LineUnknownAccount, json!({ "name": name }))
        })?,
        (None, Some(id)) => repository::account(pool, owner_id, id)
            .await?
            .ok_or_else(|| AppError::not_found(anyhow::anyhow!("no such account")))?,
        (None, None) => default_account(pool, owner_id).await?,
    };

    // A line saying "spent" filed under an income category (or the reverse) is
    // a person having typed the wrong word, and it would quietly make every
    // income report wrong.
    let flow = match spoken.flow {
        parse::Flow::Expense => Flow::Expense,
        parse::Flow::Income => Flow::Income,
    };
    match (category.flow, flow) {
        (Flow::Income, Flow::Expense) => {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "`{}` is a category of money coming in; write `+{}` for money coming in",
                category.name,
                spoken.amount
            ))
            .with_code(
                Code::LineIncomeCategory,
                json!({ "category": category.name, "amount": spoken.amount.to_string() }),
            ));
        }
        (Flow::Expense, Flow::Income) => {
            return Err(AppError::bad_request(anyhow::anyhow!(
                "`{}` is a category of money going out; leave the `+` off for money spent",
                category.name
            ))
            .with_code(Code::LineExpenseCategory, json!({ "category": category.name })));
        }
        _ => {}
    }

    // A counterparty named for the first time takes this line's category as
    // its usual one, so the next line can leave it out.
    let choice = counterparty.as_ref().map(|named| match named {
        Spoken::Known(known) => CounterpartyChoice::Existing(known.id),
        Spoken::Unknown(name) => CounterpartyChoice::New {
            name: name.clone(),
            usual_category: Some(category.id),
        },
    });
    let currency = spoken.currency.clone().unwrap_or_else(|| account.currency.clone());
    let payment = Payment {
        occurred_on,
        amount: spoken.amount,
        currency,
        rate: spoken.rate,
        description: spoken.note.clone(),
        note: spoken.note.clone(),
        counterparty: choice,
        tags: spoken.tags.clone(),
        place: None,
        pending: spoken.pending,
        flow,
        account,
        category,
    };
    Ok((payment, counterparty))
}

/// The tags of a typed line the person has never used.
async fn unknown_tags(pool: &PgPool, owner_id: Uuid, typed: &[String]) -> AppResult<Vec<String>> {
    if typed.is_empty() {
        return Ok(Vec::new());
    }
    let known = repository::tags(pool, owner_id).await?;
    Ok(typed
        .iter()
        .filter(|name| !known.iter().any(|tag| tag.name.to_lowercase() == name.to_lowercase()))
        .cloned()
        .collect())
}

/// The category a typed line is filed under: the one it names, or the usual
/// one of the counterparty it names - "this shop is groceries".
async fn category_for(pool: &PgPool, owner_id: Uuid, spoken: &parse::Spoken, counterparty: Option<&Spoken>) -> AppResult<Category> {
    match (&spoken.category, counterparty) {
        (Some(name), _) => category_named(pool, owner_id, name).await,
        (None, Some(Spoken::Known(known))) => {
            let usual = match known.default_category_id {
                Some(id) => repository::categories(pool, owner_id).await?.into_iter().find(|category| category.id == id),
                None => None,
            };
            usual.ok_or_else(|| {
                AppError::bad_request(anyhow::anyhow!(
                    "`{}` has no usual category; say what this was for, as in `{} food @{}`",
                    known.name,
                    spoken.amount,
                    known.key
                ))
                .with_code(
                    Code::LineNoUsualCategory,
                    json!({ "counterparty": known.name, "amount": spoken.amount.to_string(), "key": known.key }),
                )
            })
        }
        (None, Some(Spoken::Unknown(typed))) => Err(AppError::not_found(anyhow::anyhow!(
            "you have no counterparty called `{typed}`; say what this was for, as in `{} food @{typed}`, and it is created with that as its usual category",
            spoken.amount
        ))
        .with_code(Code::LineUnknownCounterparty, json!({ "name": typed, "amount": spoken.amount.to_string() }))),
        // The parser refuses a line that names neither.
        (None, None) => Err(AppError::bad_request(anyhow::anyhow!("the line says nothing it was for"))),
    }
}

/// The one category a typed name means.
async fn category_named(pool: &PgPool, owner_id: Uuid, name: &str) -> AppResult<Category> {
    match repository::category_by_name(pool, owner_id, name).await? {
        CategoryMatch::One(category) => Ok(*category),
        CategoryMatch::None => Err(AppError::not_found(anyhow::anyhow!("you have no category called `{name}`; create it first"))
            .with_code(Code::LineUnknownCategory, json!({ "name": name }))),
        // Resolved by asking, not by guessing: filing money under the wrong
        // one of two categories with the same name is a mistake nobody sees
        // until a report is wrong.
        CategoryMatch::Several(names) => Err(AppError::new(
            StatusCode::CONFLICT,
            anyhow::anyhow!("`{name}` means more than one category ({}); say which, as in `living/{name}`", names.join(", ")),
        )
        .with_code(Code::LineAmbiguousCategory, json!({ "name": name, "names": names.join(", ") }))),
    }
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
        Err(rest) if rest.is_empty() => Err(
            AppError::not_found(anyhow::anyhow!("you have no open account to record this against; create one first"))
                .with_code(Code::LineNoOpenAccount, json!({})),
        ),
        Err(rest) => {
            let names = rest.iter().map(|account| account.name.as_str()).collect::<Vec<_>>().join(", ");
            Err(
                AppError::bad_request(anyhow::anyhow!("say which account, as in `from {}` - you have {names}", rest[0].name))
                    .with_code(Code::LineWhichAccount, json!({ "example": rest[0].name, "accounts": names })),
            )
        }
    }
}
