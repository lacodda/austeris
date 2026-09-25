//! Asking every source for what it knows, and handing the rates on.
//!
//! A refresh asks each source that is switched on, records what answered, and
//! reports what did not - one source failing costs its own prices and nothing
//! else. In 2025 there was one source, it went down, and prices simply stopped
//! accruing with nothing saying so.
//!
//! Official exchange rates go one step further: the ledger keeps the rates it
//! values money at, so each table recorded here is also handed to it over its
//! contract (ADR 0009). The ledger never asks for them - the core knows no
//! module - which is why this side pushes.

use std::time::Duration;

use anyhow::{Context, Result};
use austeris_proto::ledger::v1::ledger_service_client::LedgerServiceClient;
use austeris_proto::ledger::v1::{Rate, RecordRatesRequest};
use chrono::{NaiveDate, NaiveTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use tonic::transport::{Channel, Endpoint};

use crate::fx::{Bcp, Cbr, RateSource, Table};
use crate::model::{Kind, SourceStatus};
use crate::repository;
use crate::source::{CoinMarketCap, PriceSource};

/// The sources this service can ask.
pub struct Sources {
    coinmarketcap: CoinMarketCap,
    bcp: Bcp,
    cbr: Cbr,
}

impl Sources {
    /// The real sources, with keys from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(CoinMarketCap::from_env(), Bcp::new(), Cbr::new())
    }

    /// A set of sources built elsewhere - pointed at recordings, in tests.
    #[must_use]
    pub fn new(coinmarketcap: CoinMarketCap, bcp: Bcp, cbr: Cbr) -> Self {
        Self { coinmarketcap, bcp, cbr }
    }

    /// The `CoinMarketCap` source, for importing its catalogue.
    #[must_use]
    pub fn coinmarketcap(&self) -> &CoinMarketCap {
        &self.coinmarketcap
    }

    /// Names the sources that are switched on.
    #[must_use]
    pub fn available(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.coinmarketcap.is_available() {
            names.push(self.coinmarketcap.name());
        }
        // Central banks need no key, so they are always on.
        names.extend([self.bcp.name(), self.cbr.name()]);
        names
    }

    /// Every source, whether it is on, and how fresh what it last said is.
    ///
    /// # Errors
    ///
    /// Returns an error when the last observations cannot be read.
    pub async fn status(&self, pool: &PgPool) -> Result<Vec<SourceStatus>> {
        let now = Utc::now();
        let mut out = Vec::new();
        let cmc_off = (!self.coinmarketcap.is_available()).then(|| "AUSTERIS_CMC_API_KEY is not set".to_owned());
        for (name, prices, off_because) in [
            (self.coinmarketcap.name(), Kind::Crypto, cmc_off),
            (self.bcp.name(), Kind::Fx, None),
            (self.cbr.name(), Kind::Fx, None),
        ] {
            let last_observed_at = repository::last_observed(pool, name).await?;
            out.push(SourceStatus {
                name: name.to_owned(),
                prices,
                available: off_because.is_none(),
                off_because,
                last_observed_at,
                stale: last_observed_at.is_none_or(|last| prices.is_stale(last, now)),
            });
        }
        Ok(out)
    }
}

/// The ledger's contract, as the one place official rates are handed to.
#[derive(Clone)]
pub struct Ledger {
    client: LedgerServiceClient<Channel>,
}

impl Ledger {
    /// A client that connects on first use.
    ///
    /// Lazy, because the market may start before the ledger answers - in one
    /// process they come up side by side - and a refresh that finds it not yet
    /// there reports a failure and tries again next time, rather than the
    /// market refusing to start.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is not one.
    pub fn connect_lazy(address: &str) -> Result<Self> {
        let channel = Endpoint::from_shared(address.to_owned())
            .with_context(|| format!("`{address}` is not the ledger's address"))?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .connect_lazy();
        Ok(Self {
            client: LedgerServiceClient::new(channel),
        })
    }

    /// Hands a table to the ledger; returns how many rates it did not have.
    async fn record(&self, source: &str, table: &Table) -> Result<u32> {
        let request = RecordRatesRequest {
            source: source.to_owned(),
            rates: table
                .rates
                .iter()
                .map(|(code, rate)| Rate {
                    base: code.clone(),
                    quote: table.home.to_owned(),
                    on_date: table.on_date.to_string(),
                    rate: rate.to_string(),
                })
                .collect(),
        };
        let response = self.client.clone().record_rates(request).await.context("handing rates to the ledger")?;
        Ok(response.into_inner().recorded)
    }
}

/// A source that was asked and did not answer.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Failure {
    /// Which source - or `ledger`, when the rates were recorded here but could
    /// not be handed on.
    pub source: String,
    /// What went wrong, as the source or the network said it.
    pub error: String,
}

/// What a refresh did.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct Refreshed {
    /// How many prices were recorded.
    pub recorded: usize,
    /// Which sources answered.
    pub sources: Vec<String>,
    /// Sources that were asked and failed - a partial refresh reports rather
    /// than pretends.
    pub failed: Vec<Failure>,
    /// Sources that cannot answer for a past day, when one was asked for.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// How many official rates the ledger did not have until now.
    pub rates_handed_on: u32,
}

impl Refreshed {
    fn failed(&mut self, source: &str, error: &anyhow::Error) {
        tracing::error!(source, error = format!("{error:#}"), "a price source failed");
        self.failed.push(Failure {
            source: source.to_owned(),
            error: format!("{error:#}"),
        });
    }
}

/// Asks every source that is on, records what they said, and hands official
/// rates to the ledger.
///
/// `on` asks for a past day; only sources that keep history can answer, and
/// the others are named in [`Refreshed::skipped`].
///
/// # Errors
///
/// Returns an error when a write to this service's own schema fails. A source
/// or the ledger failing is not an error of the refresh - it is reported in it.
pub async fn run(pool: &PgPool, sources: &Sources, ledger: Option<&Ledger>, on: Option<NaiveDate>) -> Result<Refreshed> {
    let mut refreshed = Refreshed::default();

    let cmc = &sources.coinmarketcap;
    if cmc.is_available() {
        if on.is_some() {
            refreshed.skipped.push(cmc.name().to_owned());
        } else {
            match crypto(pool, cmc).await {
                Ok(Some(recorded)) => {
                    refreshed.recorded += recorded;
                    refreshed.sources.push(cmc.name().to_owned());
                }
                Ok(None) => {}
                Err(error) => refreshed.failed(cmc.name(), &error),
            }
        }
    }

    // Today, as the server reckons it, when no day was asked for: a bank asked
    // for "today" in its own timezone's late evening answers with the table in
    // force on that day, which is the one wanted.
    let day = on.unwrap_or_else(|| Utc::now().date_naive());
    for (name, table) in [
        (sources.bcp.name(), sources.bcp.table(day).await),
        (sources.cbr.name(), sources.cbr.table(day).await),
    ] {
        let table = match table {
            Ok(table) => table,
            Err(error) => {
                refreshed.failed(name, &error);
                continue;
            }
        };

        refreshed.recorded += record_table(pool, name, &table).await?;
        refreshed.sources.push(name.to_owned());

        if let Some(ledger) = ledger {
            match ledger.record(name, &table).await {
                Ok(recorded) => refreshed.rates_handed_on += recorded,
                Err(error) => refreshed.failed("ledger", &error),
            }
        }
    }

    Ok(refreshed)
}

/// Prices every instrument bound to a crypto source. `None` when nothing is
/// bound to it, so there was nothing to ask.
async fn crypto(pool: &PgPool, source: &CoinMarketCap) -> Result<Option<usize>> {
    let bindings = repository::bindings_for(pool, source.name()).await?;
    if bindings.is_empty() {
        return Ok(None);
    }

    let external_ids: Vec<String> = bindings.iter().map(|b| b.external_id.clone()).collect();
    let quotes = source.quotes(&external_ids).await?;

    let mut recorded = 0;
    for quote in quotes {
        let Some(binding) = bindings.iter().find(|b| b.external_id == quote.external_id) else {
            // The source answered about something nobody asked for. Not an
            // error, but not ours to store either.
            continue;
        };
        repository::record_price(
            pool,
            binding.instrument_id,
            &quote.quote_currency,
            quote.observed_at,
            quote.price,
            source.name(),
        )
        .await?;
        recorded += 1;
    }
    Ok(Some(recorded))
}

/// Records a bank's table as prices of currency instruments.
///
/// A currency the bank publishes becomes an instrument the first time it is
/// seen, so a new account in a currency nobody held before has its rates
/// without anyone importing a catalogue. The observation is the start of the
/// day the table is in force for: a daily rate is a fact about a day, and
/// "as of" lookups then find it for any moment of that day.
async fn record_table(pool: &PgPool, source: &str, table: &Table) -> Result<usize> {
    let bound = repository::bindings_for(pool, source).await?;
    let observed_at = table.on_date.and_time(NaiveTime::MIN).and_utc();

    for (code, rate) in &table.rates {
        let instrument_id = if let Some(binding) = bound.iter().find(|binding| binding.external_id == *code) {
            binding.instrument_id
        } else {
            // Named by its code: the banks' own names are in Spanish and
            // Russian, and an instrument's name is shown to people who read
            // neither.
            let instrument = repository::upsert_instrument(pool, Kind::Fx, code, code, None).await?;
            repository::bind_source(pool, instrument.id, source, code, 100).await?;
            instrument.id
        };
        repository::record_price(pool, instrument_id, table.home, observed_at, *rate, source).await?;
    }
    Ok(table.rates.len())
}

/// How often the service refreshes on its own.
///
/// Hourly: the central banks publish once a day at an hour that differs by
/// country, and a crypto price older than two hours is already stale. At one
/// `CoinMarketCap` call per hundred instruments this stays well inside the
/// free plan's monthly credits.
pub const EVERY: Duration = Duration::from_hours(1);

/// Refreshes once shortly after start, then every [`EVERY`], forever.
///
/// Rates that nobody has to remember to fetch are the whole point: a screen
/// suggesting today's rate cannot rely on someone having called an endpoint
/// this morning.
pub async fn every_hour(pool: PgPool, sources: std::sync::Arc<Sources>, ledger: Option<Ledger>) {
    // Long enough for a process running every service to have its listeners
    // up, so the first hand-over to the ledger is not a failure by timing.
    tokio::time::sleep(Duration::from_secs(10)).await;
    let mut ticks = tokio::time::interval(EVERY);
    loop {
        ticks.tick().await;
        match run(&pool, &sources, ledger.as_ref(), None).await {
            Ok(refreshed) => tracing::info!(
                recorded = refreshed.recorded,
                sources = refreshed.sources.join(", "),
                failed = refreshed.failed.len(),
                rates_handed_on = refreshed.rates_handed_on,
                "prices refreshed"
            ),
            Err(error) => tracing::error!(error = format!("{error:#}"), "refreshing prices failed"),
        }
    }
}
