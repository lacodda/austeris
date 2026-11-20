//! Sorting the books out over HTTP: how many drafts wait, one change made to
//! many entries at once, and a counterparty merged into another.

use austeris_common::extract::{Json, Path};
use austeris_common::{AppError, AppResult, Caller, Code};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::tag_names;
use crate::model::Counterparty;
use crate::repository::{self, BATCH_LIMIT, Batch, BatchRefusal, MergeRefusal};

/// How many drafts wait to be sorted.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Unsorted {
    /// Entries with money not yet sorted.
    pub count: i64,
}

/// How many drafts there are: entries with money recorded before it was said
/// what it was for. What a screen shows beside the entries, so they are
/// sorted rather than forgotten; `GET /entries?unsorted=true` lists them.
#[utoipa::path(
    get,
    path = "/api/v1/ledger/entries/unsorted",
    tag = "ledger",
    responses((status = 200, description = "How many drafts wait", body = Unsorted)),
)]
pub async fn unsorted(State(pool): State<PgPool>, caller: Caller) -> AppResult<Json<Unsorted>> {
    Ok(Json(Unsorted {
        count: repository::unsorted_count(&pool, caller.id()).await?,
    }))
}

/// Money moved from one account to another.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MoveBody {
    /// The account the entries say their money moved now.
    pub from: Uuid,
    /// The account it moved, in the same currency.
    pub to: Uuid,
}

/// Labels put on and taken off.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TagBody {
    /// Labels to put on, by name; one used for the first time is created.
    #[serde(default)]
    pub add: Vec<String>,
    /// Labels to take off, by name in any case.
    #[serde(default)]
    pub remove: Vec<String>,
}

/// One change to many entries: exactly one of `categorize`, `move` and
/// `tag`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BatchBody {
    /// The entries, by id; five hundred at most.
    pub ids: Vec<Uuid>,
    /// Files their money under this category: every line not yet sorted, and
    /// every line on a category of the person's own. Lines on a category the
    /// ledger or a module keeps - an exchange's fees, a trade's - stay where
    /// they are.
    pub categorize: Option<Uuid>,
    /// Moves their money from one account to another of the same currency.
    #[serde(rename = "move")]
    pub move_between: Option<MoveBody>,
    /// Puts labels on and takes labels off.
    pub tag: Option<TagBody>,
}

/// What a batch changed.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Batched {
    /// The entries it changed.
    pub changed: Vec<Uuid>,
    /// The entries it left as they were: nothing in them was what it changes -
    /// a transfer has no category to give, an entry on another account no
    /// money to move.
    pub unchanged: Vec<Uuid>,
}

/// Makes one change to many entries, all of it or none of it: a category
/// given to a hundred drafts, their money moved off the account it was
/// recorded against by mistake, a label put on a holiday's worth. Each entry's
/// history says who changed it.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/entries/batch",
    tag = "ledger",
    request_body = BatchBody,
    responses(
        (status = 200, description = "What it changed, and what it left as it was", body = Batched),
        (status = 400, description = "It says no change or more than one, names too many entries, or a move cannot be made: to the account itself, a closed one, one in another currency, or one some of the entries already move money between", body = austeris_common::error::ErrorBody),
        (status = 404, description = "An entry, the category or an account is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Some of the entries are kept by a module, and their money changes there", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn batch(State(pool): State<PgPool>, caller: Caller, Json(body): Json<BatchBody>) -> AppResult<Json<Batched>> {
    if body.ids.is_empty() {
        return Err(AppError::bad_request(Code::BatchEmpty, anyhow::anyhow!("a batch names at least one entry")));
    }
    if body.ids.len() > BATCH_LIMIT {
        return Err(AppError::bad_request(
            Code::BatchTooLarge,
            anyhow::anyhow!("a batch names {BATCH_LIMIT} entries at most; this one names {}", body.ids.len()),
        )
        .with_params(json!({ "limit": BATCH_LIMIT, "count": body.ids.len() })));
    }
    let what = match (body.categorize, body.move_between, body.tag) {
        (Some(category), None, None) => Batch::Categorize(category),
        (None, Some(MoveBody { from, to }), None) => Batch::Move { from, to },
        (None, None, Some(TagBody { add, remove })) => {
            let (add, remove) = (tag_names(&add)?, tag_names(&remove)?);
            if add.is_empty() && remove.is_empty() {
                return Err(AppError::bad_request(
                    Code::BatchNoTags,
                    anyhow::anyhow!("say which labels to put on or take off"),
                ));
            }
            Batch::Tag { add, remove }
        }
        _ => {
            return Err(AppError::bad_request(
                Code::BatchOneChange,
                anyhow::anyhow!("a batch makes one change: `categorize`, `move` or `tag`"),
            ));
        }
    };

    let done = repository::batch(&pool, caller.id(), &caller.into(), &body.ids, &what)
        .await?
        .map_err(batch_refused)?;
    Ok(Json(Batched {
        changed: done.changed,
        unchanged: done.unchanged,
    }))
}

/// A batch the repository would not do, as the caller is told.
fn batch_refused(refusal: BatchRefusal) -> AppError {
    match refusal {
        BatchRefusal::Missing { count } => {
            AppError::not_found(Code::BatchMissing, anyhow::anyhow!("{count} of the entries are not yours")).with_params(json!({ "count": count }))
        }
        BatchRefusal::UnknownCategory => AppError::not_found(Code::CategoryNotFound, anyhow::anyhow!("no such category")),
        BatchRefusal::UnknownAccount => AppError::not_found(Code::AccountNotFound, anyhow::anyhow!("no such account")),
        BatchRefusal::SameAccount => AppError::bad_request(Code::BatchSameAccount, anyhow::anyhow!("the money is on that account already")),
        BatchRefusal::ClosedAccount { name } => {
            AppError::bad_request(Code::BatchClosedAccount, anyhow::anyhow!("{name} is closed; nothing new happens on it")).with_params(json!({ "name": name }))
        }
        BatchRefusal::OtherCurrency { from, to } => AppError::bad_request(
            Code::BatchOtherCurrency,
            anyhow::anyhow!("the money is in {from} and that account is in {to}; moving it would need a rate"),
        )
        .with_params(json!({ "from": from, "to": to })),
        BatchRefusal::OntoItself { count } => AppError::bad_request(
            Code::BatchOntoItself,
            anyhow::anyhow!("{count} of the entries move money between the two accounts already; moved, they would move it to where it came from"),
        )
        .with_params(json!({ "count": count })),
        BatchRefusal::Kept { count, module } => AppError::conflict(
            Code::BatchKept,
            anyhow::anyhow!("{count} of the entries are kept by {module}, and their money changes there"),
        )
        .with_params(json!({ "count": count, "module": module })),
    }
}

/// The counterparty another is merged into.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MergeBody {
    /// Which one.
    pub into: Uuid,
}

/// What merging a counterparty did.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Merged {
    /// The counterparty merged into, as it is now.
    pub counterparty: Counterparty,
    /// How many entries name it now instead.
    pub entries: u64,
    /// How many buttons name it now instead.
    pub templates: u64,
}

/// Merges a counterparty into another: a typo of a shop's name made a second
/// one, and its entries belong to the first. One transaction moves its
/// entries and its buttons, gives the other what it lacks of its usual
/// category, its kind and its tax number, and deletes it - and the way it was
/// typed becomes a way of typing the other, so `@supersies` finds Superseis
/// from then on.
#[utoipa::path(
    post,
    path = "/api/v1/ledger/counterparties/{id}/merge",
    tag = "ledger",
    params(("id" = Uuid, Path, description = "The counterparty merged away")),
    request_body = MergeBody,
    responses(
        (status = 200, description = "Merged", body = Merged),
        (status = 400, description = "It is merged into itself", body = austeris_common::error::ErrorBody),
        (status = 404, description = "One of the two is not yours", body = austeris_common::error::ErrorBody),
        (status = 409, description = "Both carry a tax number, and not the same one: two businesses, not one typed two ways", body = austeris_common::error::ErrorBody),
    ),
)]
pub async fn merge(State(pool): State<PgPool>, caller: Caller, Path(id): Path<Uuid>, Json(body): Json<MergeBody>) -> AppResult<Json<Merged>> {
    let merged = repository::merge_counterparty(&pool, caller.id(), &caller.into(), id, body.into)
        .await?
        .map_err(|refusal| match refusal {
            MergeRefusal::Missing => AppError::not_found(Code::CounterpartyNotFound, anyhow::anyhow!("no such counterparty")),
            MergeRefusal::Itself => AppError::bad_request(Code::CounterpartyMergeItself, anyhow::anyhow!("a counterparty is merged into another")),
            MergeRefusal::TaxIdsDiffer { from, into } => AppError::conflict(
                Code::CounterpartyMergeTaxIds,
                anyhow::anyhow!("one carries the tax number {from} and the other {into}: two businesses, not one typed two ways"),
            )
            .with_params(json!({ "from": from, "into": into })),
        })?;
    Ok(Json(Merged {
        counterparty: merged.into,
        entries: merged.entries,
        templates: merged.templates,
    }))
}
