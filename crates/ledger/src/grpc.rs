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

use austeris_common::Actor;
use austeris_proto::ledger::v1::ledger_service_server::{LedgerService, LedgerServiceServer};
use austeris_proto::ledger::v1::{
    Account as ProtoAccount, Balance, Counterparty as ProtoCounterparty, FindCounterpartyRequest, FindCounterpartyResponse, GetBalancesRequest,
    GetBalancesResponse, ListAccountsRequest, ListAccountsResponse, PostEntryRequest, PostEntryResponse, Rate as ProtoRate, RateAtRequest, RateAtResponse,
    RateInForce as ProtoRateInForce, RecordRatesRequest, RecordRatesResponse, line::Side as ProtoSide,
};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::model::AccountKind;
use crate::repository::{CounterpartyChoice, NewEntry, NewFile, NewLine, NewPlace, ObservedRate, Side};
use crate::{attachment, balance, country, parse, repository};

/// The service implementation.
pub struct Service {
    pool: PgPool,
}

impl Service {
    /// Wraps a pool as a gRPC service ready to be served.
    ///
    /// A message may be as large as a request carrying a file to the REST
    /// surface: an entry posted with the receipt it was read from carries the
    /// receipt, and tonic's own four megabytes would refuse a phone's photo.
    #[must_use]
    pub fn new(pool: PgPool) -> LedgerServiceServer<Self> {
        LedgerServiceServer::new(Self { pool }).max_decoding_message_size(attachment::BODY_LIMIT)
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

        let counterparty = request.counterparty.as_ref().map(counterparty_choice).transpose()?;
        let place = request.place.as_ref().map(place_of).transpose()?;
        let mut tags = Vec::with_capacity(request.tags.len());
        for raw in &request.tags {
            let name = parse::tag_name(raw.trim().trim_start_matches('#')).ok_or_else(|| Status::invalid_argument(format!("`{raw}` is not a tag")))?;
            if !tags.iter().any(|seen: &String| seen.to_lowercase() == name.to_lowercase()) {
                tags.push(name);
            }
        }
        let files = request.files.iter().map(file_of).collect::<Result<Vec<_>, _>>()?;

        // The module is who writes: the entry's history says so, by the name
        // it signs the entry with.
        let posted = repository::post_entry(
            &self.pool,
            owner_id,
            &Actor::module(Some(&request.source)),
            &NewEntry {
                occurred_on,
                description: request.description.trim().to_owned(),
                idempotency_key: Some(request.idempotency_key.trim().to_owned()),
                source: (!request.source.trim().is_empty()).then(|| request.source.trim().to_owned()),
                pending: request.pending,
                counterparty,
                place,
                tags,
                lines,
                files,
                allow_duplicate: false,
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
                    available: balance.available.to_string(),
                    // Empty, not "0", when there is no rate: a caller adding up
                    // a column of these must not silently include a zero for
                    // money that simply could not be converted.
                    converted: balance.converted.map(|amount| amount.to_string()).unwrap_or_default(),
                    converted_available: balance.converted_available.map(|amount| amount.to_string()).unwrap_or_default(),
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

    async fn list_accounts(&self, request: Request<ListAccountsRequest>) -> Result<Response<ListAccountsResponse>, Status> {
        let owner_id: Uuid = request
            .into_inner()
            .owner_id
            .parse()
            .map_err(|_| Status::invalid_argument("that is not a user id"))?;
        let accounts = repository::accounts(&self.pool, owner_id).await.map_err(internal("listing accounts"))?;
        Ok(Response::new(ListAccountsResponse {
            accounts: accounts
                .into_iter()
                .map(|account| ProtoAccount {
                    id: account.id.to_string(),
                    name: account.name,
                    kind: kind_name(account.kind).to_owned(),
                    currency: account.currency,
                    closed: account.closed_at.is_some(),
                })
                .collect(),
        }))
    }

    async fn find_counterparty(&self, request: Request<FindCounterpartyRequest>) -> Result<Response<FindCounterpartyResponse>, Status> {
        let request = request.into_inner();
        let owner_id: Uuid = request.owner_id.parse().map_err(|_| Status::invalid_argument("that is not a user id"))?;
        let tax_id = optional_tax_id(&request.tax_id)?;
        let found = repository::counterparty_as_named(&self.pool, owner_id, request.name.trim(), tax_id.as_deref())
            .await
            .map_err(internal("finding a counterparty"))?;
        Ok(Response::new(FindCounterpartyResponse {
            counterparty: found.map(|counterparty| ProtoCounterparty {
                id: counterparty.id.to_string(),
                name: counterparty.name,
                tax_id: counterparty.tax_id.unwrap_or_default(),
                usual_category_id: counterparty.default_category_id.map(|id| id.to_string()).unwrap_or_default(),
            }),
        }))
    }

    async fn rate_at(&self, request: Request<RateAtRequest>) -> Result<Response<RateAtResponse>, Status> {
        let request = request.into_inner();
        let (base, quote) = (currency_code(&request.base)?, currency_code(&request.quote)?);
        let on = NaiveDate::from_str(&request.on).map_err(|_| Status::invalid_argument("on is not a date (YYYY-MM-DD)"))?;
        let rate = repository::rate_in_force(&self.pool, &base, &quote, on)
            .await
            .map_err(internal("reading a rate"))?;
        Ok(Response::new(RateAtResponse {
            rate: rate.map(|rate| ProtoRateInForce {
                rate: Some(ProtoRate {
                    base: rate.base_currency,
                    quote: rate.quote_currency,
                    on_date: rate.on_date.to_string(),
                    rate: rate.rate.to_string(),
                }),
                source: rate.source,
                stale: rate.stale,
            }),
        }))
    }
}

/// The counterparty a module names, as the repository finds or creates it.
fn counterparty_choice(named: &austeris_proto::ledger::v1::CounterpartyName) -> Result<CounterpartyChoice, Status> {
    let name = named.name.trim();
    let tax_id = optional_tax_id(&named.tax_id)?;
    if name.is_empty() {
        return Err(Status::invalid_argument("a counterparty is named; one without a name could never be typed"));
    }
    let usual_category = if named.usual_category_id.trim().is_empty() {
        None
    } else {
        Some(parse_id(named.usual_category_id.trim(), "category")?)
    };
    Ok(CounterpartyChoice::New {
        name: name.to_owned(),
        usual_category,
        tax_id,
    })
}

/// A tax number as it is kept, or `None` for an empty one.
fn optional_tax_id(raw: &str) -> Result<Option<String>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    crate::model::tax_id(raw)
        .map(Some)
        .ok_or_else(|| Status::invalid_argument(format!("`{raw}` is not a tax number")))
}

/// A place a module names, with its country checked.
fn place_of(place: &austeris_proto::ledger::v1::Place) -> Result<NewPlace, Status> {
    let country = country::code(&place.country).ok_or_else(|| Status::invalid_argument(format!("`{}` is not a country code", place.country)))?;
    let city = Some(place.city.trim()).filter(|city| !city.is_empty()).map(ToOwned::to_owned);
    Ok(NewPlace { country, city })
}

/// A file a module keeps with the entry, taken for what its bytes are.
fn file_of(file: &austeris_proto::ledger::v1::File) -> Result<NewFile, Status> {
    let name = attachment::file_name(&file.name).ok_or_else(|| Status::invalid_argument("a file needs a name"))?;
    if file.content.is_empty() {
        return Err(Status::invalid_argument(format!("{name} is empty")));
    }
    if file.content.len() > attachment::MAX_BYTES {
        return Err(Status::invalid_argument(format!(
            "{name} is larger than the {} MB a file kept with an entry can be",
            attachment::MAX_BYTES / (1024 * 1024)
        )));
    }
    let media_type =
        attachment::media_type(&file.content).ok_or_else(|| Status::invalid_argument(format!("{name} is not a picture, a PDF or an XML document")))?;
    Ok(NewFile {
        name,
        media_type: media_type.to_owned(),
        content: file.content.clone(),
    })
}

/// An account's kind, as the contract spells it.
fn kind_name(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::Cash => "cash",
        AccountKind::Bank => "bank",
        AccountKind::Card => "card",
        AccountKind::Brokerage => "brokerage",
        AccountKind::CryptoWallet => "crypto_wallet",
        AccountKind::Deposit => "deposit",
        AccountKind::Loan => "loan",
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
    // A usual category, or a counterparty, that is not this person's: the
    // keys name the owner, so the database refuses it as missing.
    let not_theirs = error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|error| error.as_database_error())
        .any(|db| db.code().as_deref() == Some("23503"));
    if not_theirs {
        return Status::invalid_argument("the entry names a category or a counterparty that is not this person's");
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
