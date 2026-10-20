//! The services this module asks: the ledger, for the books it posts into, and
//! identity, for whose a folder is.
//!
//! Each is a trait with one implementation that calls the service over its
//! contract. The trait is what the rest of the module is written against, so
//! the tests can hand it the ledger's own gRPC service in the same process -
//! the contract's real answers, without a socket.

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use austeris_proto::identity::v1::FindPersonRequest;
use austeris_proto::identity::v1::identity_service_client::IdentityServiceClient;
use austeris_proto::ledger::v1::ledger_service_client::LedgerServiceClient;
use austeris_proto::ledger::v1::{FindCounterpartyRequest, ListAccountsRequest, PostEntryRequest, RateAtRequest};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use tonic::Status;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

/// An account of the person's, as the ledger lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Which one.
    pub id: Uuid,
    /// What the person calls it.
    pub name: String,
    /// `cash`, `bank`, `card`...
    pub kind: String,
    /// The currency it is in.
    pub currency: String,
    /// Closed: nothing new is recorded on it.
    pub closed: bool,
}

/// A counterparty the person has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counterparty {
    /// Which one.
    pub id: Uuid,
    /// What the person calls it.
    pub name: String,
    /// What money spent there is usually for.
    pub usual_category: Option<Uuid>,
}

/// A rate in force on a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    /// How many of the quote currency one of the base buys.
    pub rate: Decimal,
    /// The day it was set for.
    pub on_date: NaiveDate,
    /// Older than a long weekend by the day asked about.
    pub stale: bool,
}

/// What posting an entry came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posted {
    /// The entry.
    pub entry_id: Uuid,
    /// Whether this call wrote it, or its key had written it already.
    pub created: bool,
}

/// The ledger, as this module uses it.
pub trait Books: Send + Sync {
    /// The person's accounts, open ones first.
    fn accounts(&self, owner: Uuid) -> impl Future<Output = Result<Vec<Account>, Status>> + Send;
    /// The counterparty a document names, by tax number or name.
    fn counterparty(&self, owner: Uuid, tax_id: Option<&str>, name: &str) -> impl Future<Output = Result<Option<Counterparty>, Status>> + Send;
    /// The rate from `base` into `quote` in force on a day.
    fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> impl Future<Output = Result<Option<Rate>, Status>> + Send;
    /// Posts an entry.
    fn post(&self, request: PostEntryRequest) -> impl Future<Output = Result<Posted, Status>> + Send;
}

/// Who signs in with an address, as this module asks it.
pub trait People: Send + Sync {
    /// The person signing in with `email`, while they can.
    fn person(&self, email: &str) -> impl Future<Output = Result<Option<Uuid>, Status>> + Send;
}

/// A service reached over its contract.
#[derive(Clone)]
pub struct Remote {
    channel: Channel,
}

impl Remote {
    /// A client that connects on first use.
    ///
    /// Lazy, because this module may start before the service it calls
    /// answers: a document arriving in that moment waits and is tried again,
    /// rather than the module refusing to start.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is not one.
    pub fn connect_lazy(address: &str) -> Result<Self> {
        let channel = Endpoint::from_shared(address.to_owned())
            .with_context(|| format!("`{address}` is not an address"))?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .connect_lazy();
        Ok(Self { channel })
    }

    fn ledger(&self) -> LedgerServiceClient<Channel> {
        // A receipt's file travels with its entry, so a request may be as
        // large as a file kept with one.
        LedgerServiceClient::new(self.channel.clone()).max_encoding_message_size(austeris_ledger::attachment::BODY_LIMIT)
    }
}

impl Books for Remote {
    async fn accounts(&self, owner: Uuid) -> Result<Vec<Account>, Status> {
        let response = self
            .ledger()
            .list_accounts(ListAccountsRequest { owner_id: owner.to_string() })
            .await?
            .into_inner();
        response
            .accounts
            .into_iter()
            .map(|account| {
                Ok(Account {
                    id: id(&account.id)?,
                    name: account.name,
                    kind: account.kind,
                    currency: account.currency,
                    closed: account.closed,
                })
            })
            .collect()
    }

    async fn counterparty(&self, owner: Uuid, tax_id: Option<&str>, name: &str) -> Result<Option<Counterparty>, Status> {
        let response = self
            .ledger()
            .find_counterparty(FindCounterpartyRequest {
                owner_id: owner.to_string(),
                tax_id: tax_id.unwrap_or_default().to_owned(),
                name: name.to_owned(),
            })
            .await?
            .into_inner();
        response.counterparty.map(counterparty).transpose()
    }

    async fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> Result<Option<Rate>, Status> {
        let response = self
            .ledger()
            .rate_at(RateAtRequest {
                base: base.to_owned(),
                quote: quote.to_owned(),
                on: on.to_string(),
            })
            .await?
            .into_inner();
        response.rate.map(rate).transpose()
    }

    async fn post(&self, request: PostEntryRequest) -> Result<Posted, Status> {
        let response = self.ledger().post_entry(request).await?.into_inner();
        Ok(Posted {
            entry_id: id(&response.entry_id)?,
            created: response.created,
        })
    }
}

impl People for Remote {
    async fn person(&self, email: &str) -> Result<Option<Uuid>, Status> {
        let response = IdentityServiceClient::new(self.channel.clone())
            .find_person(FindPersonRequest { email: email.to_owned() })
            .await?
            .into_inner();
        if response.user_id.is_empty() {
            Ok(None)
        } else {
            id(&response.user_id).map(Some)
        }
    }
}

/// A counterparty as the contract carries it.
///
/// # Errors
///
/// Returns an error when the ledger answered with an id that is not one.
pub fn counterparty(found: austeris_proto::ledger::v1::Counterparty) -> Result<Counterparty, Status> {
    Ok(Counterparty {
        id: id(&found.id)?,
        name: found.name,
        usual_category: if found.usual_category_id.is_empty() {
            None
        } else {
            Some(id(&found.usual_category_id)?)
        },
    })
}

/// A rate as the contract carries it.
///
/// # Errors
///
/// Returns an error when the ledger answered with a rate or a day that is not
/// one.
pub fn rate(found: austeris_proto::ledger::v1::RateInForce) -> Result<Rate, Status> {
    let stated = found.rate.ok_or_else(|| Status::internal("the ledger answered a rate without one"))?;
    Ok(Rate {
        rate: Decimal::from_str(&stated.rate).map_err(|_| Status::internal(format!("the ledger's rate `{}` is not a number", stated.rate)))?,
        on_date: NaiveDate::from_str(&stated.on_date).map_err(|_| Status::internal(format!("the ledger's day `{}` is not a date", stated.on_date)))?,
        stale: found.stale,
    })
}

/// An id the other side answered with.
///
/// # Errors
///
/// Returns an error when it is not one - the other side's fault, not the
/// caller's.
pub fn id(raw: &str) -> Result<Uuid, Status> {
    raw.parse().map_err(|_| Status::internal(format!("`{raw}` came back as an id")))
}
