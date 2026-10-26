//! The services this module asks: the ledger, for the accounts trades happen
//! on and the money they move, and the market, for what instruments are and
//! what they are worth.
//!
//! Each is a trait the rest of the module is written against, with two
//! implementations: one that calls the service over its contract, and one that
//! calls a service's own implementation of that contract in this process -
//! what the tests and the demo use, to get the contract's real answers
//! without a socket.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use austeris_proto::ledger::v1::ledger_service_client::LedgerServiceClient;
use austeris_proto::ledger::v1::ledger_service_server::LedgerService;
use austeris_proto::ledger::v1::{GetBalancesRequest, ListAccountsRequest, PostEntryRequest, RateAtRequest, RestateEntryRequest, WithdrawEntryRequest};
use austeris_proto::market::v1::market_service_client::MarketServiceClient;
use austeris_proto::market::v1::market_service_server::MarketService;
use austeris_proto::market::v1::{GetInstrumentsRequest, GetPriceAtRequest, GetPricesRequest};
use chrono::{DateTime, NaiveDate, Utc};
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
    /// `brokerage`, `crypto_wallet`, `bank`...
    pub kind: String,
    /// The currency its money is in.
    pub currency: String,
    /// Closed: nothing new happens on it.
    pub closed: bool,
}

impl Account {
    /// Whether positions are held on it: a broker's or a wallet's.
    #[must_use]
    pub fn holds_positions(&self) -> bool {
        crate::HOLDING_KINDS.contains(&self.kind.as_str())
    }
}

/// The money on an account at the end of a day, as the ledger adds it up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cash {
    /// Which account.
    pub account_id: Uuid,
    /// How much, in the account's currency.
    pub amount: Decimal,
    /// The same in the currency asked for, when the ledger had a rate.
    pub converted: Option<Decimal>,
    /// The day the rate behind `converted` was set for.
    pub rate_on: Option<NaiveDate>,
    /// That rate was older than a long weekend.
    pub stale: bool,
}

/// A rate in force on a day, as the ledger keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// How many of the quote currency one of the base buys.
    pub rate: Decimal,
    /// Older than a long weekend by the day asked about.
    pub stale: bool,
}

/// An instrument, as the market knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    /// Which one.
    pub id: Uuid,
    /// `crypto`, `stock`, `fund`, `fx`...
    pub kind: String,
    /// Its ticker.
    pub symbol: String,
    /// What it is called.
    pub name: String,
}

/// What an instrument was worth, as the market last heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    /// Which instrument.
    pub instrument_id: Uuid,
    /// What a unit was worth, in the currency asked for.
    pub price: Decimal,
    /// When it was observed.
    pub observed_at: DateTime<Utc>,
    /// Older than its kind stays current for at the moment asked about.
    pub stale: bool,
}

/// The ledger, as this module uses it.
pub trait Books: Send + Sync {
    /// The person's accounts, open ones first.
    fn accounts(&self, owner: Uuid) -> impl Future<Output = Result<Vec<Account>, Status>> + Send;
    /// The money on every account at the end of `as_of`, converted into
    /// `currency` where a rate is known.
    fn cash(&self, owner: Uuid, as_of: NaiveDate, currency: &str) -> impl Future<Output = Result<Vec<Cash>, Status>> + Send;
    /// Posts an entry, and says which.
    fn post(&self, request: PostEntryRequest) -> impl Future<Output = Result<Uuid, Status>> + Send;
    /// Restates an entry this module keeps.
    fn restate(&self, request: RestateEntryRequest) -> impl Future<Output = Result<Uuid, Status>> + Send;
    /// Takes back an entry this module keeps; `false` when it was gone.
    fn withdraw(&self, owner: Uuid, key: &str) -> impl Future<Output = Result<bool, Status>> + Send;
    /// The rate from `base` into `quote` in force on a day, when one is known.
    fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> impl Future<Output = Result<Option<Rate>, Status>> + Send;
}

/// The market, as this module uses it.
pub trait Market: Send + Sync {
    /// What each instrument named is; one nobody knows is left out.
    fn instruments(&self, ids: &[Uuid]) -> impl Future<Output = Result<Vec<Instrument>, Status>> + Send;
    /// The latest price of each, in `currency`; one with none is left out.
    fn prices(&self, ids: &[Uuid], currency: &str) -> impl Future<Output = Result<Vec<Price>, Status>> + Send;
    /// The price of one as of an instant.
    fn price_at(&self, id: Uuid, currency: &str, at: DateTime<Utc>) -> impl Future<Output = Result<Option<Price>, Status>> + Send;
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
    /// answers - in one process they come up side by side - and a request in
    /// that moment is refused as the service being unavailable rather than
    /// the module refusing to start.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is not one.
    pub fn connect_lazy(address: &str) -> Result<Self> {
        let channel = Endpoint::from_shared(address.to_owned())
            .with_context(|| format!("`{address}` is not an address"))?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .connect_lazy();
        Ok(Self { channel })
    }
}

/// A service's own implementation of its contract, called in this process.
pub struct Local<T>(pub Arc<T>);

impl<T> Clone for Local<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Books for Remote {
    async fn accounts(&self, owner: Uuid) -> Result<Vec<Account>, Status> {
        let listed = LedgerServiceClient::new(self.channel.clone())
            .list_accounts(ListAccountsRequest { owner_id: owner.to_string() })
            .await?
            .into_inner();
        accounts(listed)
    }

    async fn cash(&self, owner: Uuid, as_of: NaiveDate, currency: &str) -> Result<Vec<Cash>, Status> {
        let balances = LedgerServiceClient::new(self.channel.clone())
            .get_balances(balances_request(owner, as_of, currency))
            .await?
            .into_inner();
        cash(balances)
    }

    async fn post(&self, request: PostEntryRequest) -> Result<Uuid, Status> {
        let posted = LedgerServiceClient::new(self.channel.clone()).post_entry(request).await?.into_inner();
        id(&posted.entry_id)
    }

    async fn restate(&self, request: RestateEntryRequest) -> Result<Uuid, Status> {
        let restated = LedgerServiceClient::new(self.channel.clone()).restate_entry(request).await?.into_inner();
        id(&restated.entry_id)
    }

    async fn withdraw(&self, owner: Uuid, key: &str) -> Result<bool, Status> {
        let withdrawn = LedgerServiceClient::new(self.channel.clone())
            .withdraw_entry(withdraw_request(owner, key))
            .await?
            .into_inner();
        Ok(withdrawn.withdrawn)
    }

    async fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> Result<Option<Rate>, Status> {
        let found = LedgerServiceClient::new(self.channel.clone())
            .rate_at(rate_request(base, quote, on))
            .await?
            .into_inner();
        found.rate.as_ref().map(rate).transpose()
    }
}

impl<T: LedgerService> Books for Local<T> {
    async fn accounts(&self, owner: Uuid) -> Result<Vec<Account>, Status> {
        let listed = self
            .0
            .list_accounts(tonic::Request::new(ListAccountsRequest { owner_id: owner.to_string() }))
            .await?
            .into_inner();
        accounts(listed)
    }

    async fn cash(&self, owner: Uuid, as_of: NaiveDate, currency: &str) -> Result<Vec<Cash>, Status> {
        let balances = self
            .0
            .get_balances(tonic::Request::new(balances_request(owner, as_of, currency)))
            .await?
            .into_inner();
        cash(balances)
    }

    async fn post(&self, request: PostEntryRequest) -> Result<Uuid, Status> {
        let posted = self.0.post_entry(tonic::Request::new(request)).await?.into_inner();
        id(&posted.entry_id)
    }

    async fn restate(&self, request: RestateEntryRequest) -> Result<Uuid, Status> {
        let restated = self.0.restate_entry(tonic::Request::new(request)).await?.into_inner();
        id(&restated.entry_id)
    }

    async fn withdraw(&self, owner: Uuid, key: &str) -> Result<bool, Status> {
        let withdrawn = self.0.withdraw_entry(tonic::Request::new(withdraw_request(owner, key))).await?.into_inner();
        Ok(withdrawn.withdrawn)
    }

    async fn rate(&self, base: &str, quote: &str, on: NaiveDate) -> Result<Option<Rate>, Status> {
        let found = self.0.rate_at(tonic::Request::new(rate_request(base, quote, on))).await?.into_inner();
        found.rate.as_ref().map(rate).transpose()
    }
}

impl Market for Remote {
    async fn instruments(&self, ids: &[Uuid]) -> Result<Vec<Instrument>, Status> {
        let found = MarketServiceClient::new(self.channel.clone())
            .get_instruments(instruments_request(ids))
            .await?
            .into_inner();
        instruments(found)
    }

    async fn prices(&self, ids: &[Uuid], currency: &str) -> Result<Vec<Price>, Status> {
        let found = MarketServiceClient::new(self.channel.clone())
            .get_prices(prices_request(ids, currency))
            .await?
            .into_inner();
        found.prices.iter().map(price).collect()
    }

    async fn price_at(&self, id: Uuid, currency: &str, at: DateTime<Utc>) -> Result<Option<Price>, Status> {
        let found = MarketServiceClient::new(self.channel.clone())
            .get_price_at(price_at_request(id, currency, at))
            .await?
            .into_inner();
        found.price.as_ref().map(price).transpose()
    }
}

impl<T: MarketService> Market for Local<T> {
    async fn instruments(&self, ids: &[Uuid]) -> Result<Vec<Instrument>, Status> {
        let found = self.0.get_instruments(tonic::Request::new(instruments_request(ids))).await?.into_inner();
        instruments(found)
    }

    async fn prices(&self, ids: &[Uuid], currency: &str) -> Result<Vec<Price>, Status> {
        let found = self.0.get_prices(tonic::Request::new(prices_request(ids, currency))).await?.into_inner();
        found.prices.iter().map(price).collect()
    }

    async fn price_at(&self, id: Uuid, currency: &str, at: DateTime<Utc>) -> Result<Option<Price>, Status> {
        let found = self.0.get_price_at(tonic::Request::new(price_at_request(id, currency, at))).await?.into_inner();
        found.price.as_ref().map(price).transpose()
    }
}

fn balances_request(owner: Uuid, as_of: NaiveDate, currency: &str) -> GetBalancesRequest {
    GetBalancesRequest {
        owner_id: owner.to_string(),
        as_of: as_of.to_string(),
        in_currency: currency.to_owned(),
    }
}

fn withdraw_request(owner: Uuid, key: &str) -> WithdrawEntryRequest {
    WithdrawEntryRequest {
        owner_id: owner.to_string(),
        idempotency_key: key.to_owned(),
        source: crate::SOURCE.to_owned(),
    }
}

fn rate_request(base: &str, quote: &str, on: NaiveDate) -> RateAtRequest {
    RateAtRequest {
        base: base.to_owned(),
        quote: quote.to_owned(),
        on: on.to_string(),
    }
}

fn instruments_request(ids: &[Uuid]) -> GetInstrumentsRequest {
    GetInstrumentsRequest {
        instrument_ids: ids.iter().map(Uuid::to_string).collect(),
    }
}

fn prices_request(ids: &[Uuid], currency: &str) -> GetPricesRequest {
    GetPricesRequest {
        instrument_ids: ids.iter().map(Uuid::to_string).collect(),
        quote_currency: currency.to_owned(),
    }
}

fn price_at_request(id: Uuid, currency: &str, at: DateTime<Utc>) -> GetPriceAtRequest {
    GetPriceAtRequest {
        instrument_id: id.to_string(),
        quote_currency: currency.to_owned(),
        at_unix_seconds: at.timestamp(),
    }
}

fn accounts(listed: austeris_proto::ledger::v1::ListAccountsResponse) -> Result<Vec<Account>, Status> {
    listed
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

fn cash(balances: austeris_proto::ledger::v1::GetBalancesResponse) -> Result<Vec<Cash>, Status> {
    balances
        .balances
        .into_iter()
        .map(|balance| {
            Ok(Cash {
                account_id: id(&balance.account_id)?,
                amount: number(&balance.amount)?,
                // Empty, never zero, when there was no rate: money that could
                // not be converted is not money worth nothing.
                converted: optional(&balance.converted, number)?,
                rate_on: optional(&balance.rate_on, |day| {
                    NaiveDate::from_str(day).map_err(|_| Status::internal(format!("the ledger's day `{day}` is not a date")))
                })?,
                stale: balance.rate_stale,
            })
        })
        .collect()
}

fn rate(found: &austeris_proto::ledger::v1::RateInForce) -> Result<Rate, Status> {
    let stated = found.rate.as_ref().ok_or_else(|| Status::internal("the ledger answered a rate without one"))?;
    Ok(Rate {
        rate: number(&stated.rate)?,
        stale: found.stale,
    })
}

fn instruments(found: austeris_proto::market::v1::GetInstrumentsResponse) -> Result<Vec<Instrument>, Status> {
    found
        .instruments
        .into_iter()
        .map(|instrument| {
            Ok(Instrument {
                id: id(&instrument.id)?,
                kind: instrument.kind,
                symbol: instrument.symbol,
                name: instrument.name,
            })
        })
        .collect()
}

fn price(found: &austeris_proto::market::v1::Price) -> Result<Price, Status> {
    Ok(Price {
        instrument_id: id(&found.instrument_id)?,
        price: number(&found.price)?,
        observed_at: DateTime::from_timestamp(found.observed_at_unix_seconds, 0)
            .ok_or_else(|| Status::internal("the market answered with a moment that is not one"))?,
        stale: found.stale,
    })
}

/// A field the other side may leave empty.
fn optional<T>(raw: &str, read: impl Fn(&str) -> Result<T, Status>) -> Result<Option<T>, Status> {
    if raw.is_empty() { Ok(None) } else { read(raw).map(Some) }
}

/// A decimal the other side answered with.
fn number(raw: &str) -> Result<Decimal, Status> {
    Decimal::from_str(raw).map_err(|_| Status::internal(format!("`{raw}` came back as a number")))
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
