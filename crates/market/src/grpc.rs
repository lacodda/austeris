//! The market service's gRPC surface.
//!
//! What every other service asks: what is this worth, now or then. Prices cross
//! the wire as strings, because protobuf has no decimal type and a price that
//! travels as a double is not the price that was recorded (ADR 0004).

use austeris_proto::market::v1::market_server::{Market, MarketServer};
use austeris_proto::market::v1::{GetPriceAtRequest, GetPriceAtResponse, GetPricesRequest, GetPricesResponse, Price};
use chrono::DateTime;
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::repository;

/// The service implementation.
pub struct Service {
    pool: PgPool,
}

impl Service {
    /// Wraps a pool as a gRPC service ready to be served.
    #[must_use]
    pub fn new(pool: PgPool) -> MarketServer<Self> {
        MarketServer::new(Self { pool })
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
impl Market for Service {
    async fn get_prices(&self, request: Request<GetPricesRequest>) -> Result<Response<GetPricesResponse>, Status> {
        let request = request.into_inner();

        // An unparseable id is the caller's mistake, and saying which one is
        // wrong beats an empty answer they cannot explain.
        let ids: Vec<Uuid> = request
            .instrument_ids
            .iter()
            .map(|id| id.parse().map_err(|_| Status::invalid_argument(format!("`{id}` is not an instrument id"))))
            .collect::<Result<_, _>>()?;

        let prices = repository::latest_prices(&self.pool, &ids, &request.quote_currency)
            .await
            .map_err(internal("reading the latest prices"))?;

        Ok(Response::new(GetPricesResponse {
            prices: prices.iter().map(into_proto).collect(),
        }))
    }

    async fn get_price_at(&self, request: Request<GetPriceAtRequest>) -> Result<Response<GetPriceAtResponse>, Status> {
        let request = request.into_inner();

        let id: Uuid = request
            .instrument_id
            .parse()
            .map_err(|_| Status::invalid_argument("that is not an instrument id"))?;
        let at = DateTime::from_timestamp(request.at_unix_seconds, 0).ok_or_else(|| Status::invalid_argument("that is not a moment in time"))?;

        let price = repository::price_at(&self.pool, id, &request.quote_currency, at)
            .await
            .map_err(internal("reading a price"))?;

        Ok(Response::new(GetPriceAtResponse {
            price: price.as_ref().map(into_proto),
        }))
    }
}

/// Turns a stored price into its wire form.
fn into_proto(price: &crate::model::Price) -> Price {
    Price {
        instrument_id: price.instrument_id.to_string(),
        quote_currency: price.quote_currency.clone(),
        // A string, deliberately: every digit the source sent survives the hop.
        price: price.price.to_string(),
        observed_at_unix_seconds: price.observed_at.timestamp(),
        source: price.source.clone(),
    }
}

/// Logs a failure here and tells the caller only that it happened.
fn internal(what: &'static str) -> impl Fn(anyhow::Error) -> Status {
    move |error| {
        tracing::error!(%error, "{what} failed");
        Status::internal(what)
    }
}
