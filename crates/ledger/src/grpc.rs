//! The ledger service's gRPC surface.
//!
//! This is the one way a module turns what it knows into money in the books: a
//! loan schedule works out that an instalment fell due, a salary run that pay
//! arrived, and each says so here in the ledger's own terms. The ledger learns
//! nothing about loans or salaries in the process (ADR 0001).
//!
//! Every amount crosses as a string, because protobuf has no decimal type and
//! money that travels as a double is not the money anyone recorded (ADR 0004).

use std::str::FromStr;

use austeris_proto::ledger::v1::ledger_service_server::{LedgerService, LedgerServiceServer};
use austeris_proto::ledger::v1::{
    Balance, GetBalancesRequest, GetBalancesResponse, PostEntryRequest, PostEntryResponse, RecordRatesRequest, RecordRatesResponse, line::Side as ProtoSide,
};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::repository::{NewEntry, NewLine, ObservedRate, Side};
use crate::{balance, repository};

/// The service implementation.
pub struct Service {
    pool: PgPool,
}

impl Service {
    /// Wraps a pool as a gRPC service ready to be served.
    #[must_use]
    pub fn new(pool: PgPool) -> LedgerServiceServer<Self> {
        LedgerServiceServer::new(Self { pool })
    }

    /// The bare service, so tests can call the contract without a socket.
    ///
    /// What is worth testing here is the answers the contract gives, not
    /// tonic's transport.
    #[must_use]
    pub fn for_tests(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl LedgerService for Service {
    async fn post_entry(&self, request: Request<PostEntryRequest>) -> Result<Response<PostEntryResponse>, Status> {
        let request = request.into_inner();

        let owner_id: Uuid = request.owner_id.parse().map_err(|_| Status::invalid_argument("that is not a user id"))?;

        // Required of a module, unlike an entry a person types: a module
        // retries, and without a key two attempts at one salary are two
        // salaries. Refusing here is what makes "at least once" delivery safe
        // for every caller, rather than each module inventing its own guard.
        if request.idempotency_key.trim().is_empty() {
            return Err(Status::invalid_argument(
                "a module posting an entry names an idempotency key, so a retry does not book twice",
            ));
        }

        let occurred_on = NaiveDate::from_str(&request.occurred_on).map_err(|_| Status::invalid_argument("occurred_on is not a date (YYYY-MM-DD)"))?;

        if request.lines.len() < 2 {
            return Err(Status::invalid_argument(
                "an entry has at least two sides: money leaves somewhere and arrives somewhere",
            ));
        }

        let mut lines = Vec::with_capacity(request.lines.len());
        for line in &request.lines {
            let amount = Decimal::from_str(&line.amount).map_err(|_| Status::invalid_argument(format!("`{}` is not an amount", line.amount)))?;
            if amount.is_zero() {
                return Err(Status::invalid_argument("a line of zero records nothing"));
            }

            let side = match &line.side {
                Some(ProtoSide::AccountId(id)) => Side::Account(parse_id(id, "account")?),
                Some(ProtoSide::CategoryId(id)) => Side::Category(parse_id(id, "category")?),
                Some(ProtoSide::Conversion(_)) => Side::Conversion,
                None => {
                    return Err(Status::invalid_argument(
                        "a line names an account, a category or a conversion; one that names none is money from nowhere",
                    ));
                }
            };

            lines.push(NewLine {
                side,
                amount,
                currency: currency_code(&line.currency)?,
                note: line.note.trim().to_owned(),
            });
        }

        let posted = repository::post_entry(
            &self.pool,
            owner_id,
            &NewEntry {
                occurred_on,
                description: request.description.trim().to_owned(),
                idempotency_key: Some(request.idempotency_key.trim().to_owned()),
                source: (!request.source.trim().is_empty()).then(|| request.source.trim().to_owned()),
                lines,
            },
        )
        .await
        .map_err(|error| refused_or_internal(&error))?;

        Ok(Response::new(PostEntryResponse {
            entry_id: posted.entry.id.to_string(),
            created: posted.created,
        }))
    }

    async fn get_balances(&self, request: Request<GetBalancesRequest>) -> Result<Response<GetBalancesResponse>, Status> {
        let request = request.into_inner();

        let owner_id: Uuid = request.owner_id.parse().map_err(|_| Status::invalid_argument("that is not a user id"))?;

        let as_of = if request.as_of.trim().is_empty() {
            Utc::now().date_naive()
        } else {
            NaiveDate::from_str(&request.as_of).map_err(|_| Status::invalid_argument("as_of is not a date (YYYY-MM-DD)"))?
        };

        let mut balances = repository::balances(&self.pool, owner_id, as_of).await.map_err(internal("reading balances"))?;

        if !request.in_currency.trim().is_empty() {
            let currency = currency_code(&request.in_currency)?;
            balance::convert(&self.pool, &mut balances, &currency, as_of)
                .await
                .map_err(internal("converting balances"))?;
        }

        Ok(Response::new(GetBalancesResponse {
            balances: balances
                .iter()
                .map(|balance| Balance {
                    account_id: balance.account_id.to_string(),
                    name: balance.name.clone(),
                    currency: balance.currency.clone(),
                    amount: balance.amount.to_string(),
                    // Empty, not "0", when there is no rate: a caller adding up
                    // a column of these must not silently include a zero for
                    // money that simply could not be converted.
                    converted: balance.converted.map(|amount| amount.to_string()).unwrap_or_default(),
                    rate_on: balance.rate_used.as_ref().map(|rate| rate.on_date.to_string()).unwrap_or_default(),
                    rate_stale: balance.rate_used.as_ref().is_some_and(|rate| rate.stale),
                })
                .collect(),
        }))
    }

    async fn record_rates(&self, request: Request<RecordRatesRequest>) -> Result<Response<RecordRatesResponse>, Status> {
        let request = request.into_inner();

        let source = request.source.trim();
        // A rate with no source cannot be told apart from one a person typed,
        // and those are kept by different rules.
        if source.is_empty() || source.eq_ignore_ascii_case("manual") {
            return Err(Status::invalid_argument("rates from a module name the source that published them"));
        }

        let mut rates = Vec::with_capacity(request.rates.len());
        for rate in &request.rates {
            let (base, quote) = (currency_code(&rate.base)?, currency_code(&rate.quote)?);
            if base == quote {
                return Err(Status::invalid_argument(format!("a rate of {base} in itself is one, and is not recorded")));
            }
            let value = Decimal::from_str(&rate.rate).map_err(|_| Status::invalid_argument(format!("`{}` is not a rate", rate.rate)))?;
            if !value.is_sign_positive() || value.is_zero() {
                return Err(Status::invalid_argument(format!("a rate is positive; {base}->{quote} is {value}")));
            }
            let on_date = NaiveDate::from_str(&rate.on_date).map_err(|_| Status::invalid_argument("on_date is not a date (YYYY-MM-DD)"))?;
            rates.push(ObservedRate {
                base,
                quote,
                on_date,
                rate: value,
            });
        }

        let recorded = repository::record_observed_rates(&self.pool, source, &rates)
            .await
            .map_err(internal("recording rates"))?;

        Ok(Response::new(RecordRatesResponse {
            recorded: u32::try_from(recorded).unwrap_or(u32::MAX),
        }))
    }
}

/// An id, or which kind of id it was supposed to be.
fn parse_id(raw: &str, what: &str) -> Result<Uuid, Status> {
    raw.parse().map_err(|_| Status::invalid_argument(format!("`{raw}` is not a {what} id")))
}

/// A currency code, normalised the same way the REST surface does it.
fn currency_code(raw: &str) -> Result<String, Status> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(Status::invalid_argument(format!("`{raw}` is not a currency code")));
    }
    Ok(trimmed.to_uppercase())
}

/// Tells a module what it got wrong, and hides what it did not.
///
/// An entry the schema's rules refuse and one naming somebody else's account
/// are both the caller's mistakes, and a module retrying an `internal` forever
/// because its own arithmetic is wrong is the failure this avoids. A rule is
/// recognised by its SQLSTATE, so one added to the schema later is refused
/// here without being listed.
fn refused_or_internal(error: &anyhow::Error) -> Status {
    let refused = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .find(|db| db.code().as_deref() == Some("23514"))
        .map(|db| db.message().to_owned());
    if let Some(message) = refused {
        return Status::invalid_argument(message);
    }

    let text = format!("{error:#}");
    if text.contains("does not exist") {
        Status::invalid_argument(text)
    } else {
        tracing::error!(%error, "posting an entry failed");
        Status::internal("posting an entry")
    }
}

/// Logs a failure here and tells the caller only that it happened.
fn internal(what: &'static str) -> impl Fn(anyhow::Error) -> Status {
    move |error| {
        tracing::error!(%error, "{what} failed");
        Status::internal(what)
    }
}
