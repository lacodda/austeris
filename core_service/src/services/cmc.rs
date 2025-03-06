use crate::models::cmc::{CmcListing, CmcQuote, CmcQuoteResponse, CmcResponse};
use anyhow::{anyhow, Result};
use sqlx::PgPool;
use std::env;

#[derive(Clone)]
pub struct CmcService {
    client: reqwest::Client,
    api_key: String,
}

impl CmcService {
    pub fn new() -> Self {
        let api_key = env::var("COINMARKETCAP_API_KEY").expect("COINMARKETCAP_API_KEY must be set");
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    pub async fn fetch_cmc_listings(&self) -> Result<Vec<CmcListing>> {
        let response = self
            .client
            .get("https://pro-api.coinmarketcap.com/v1/cryptocurrency/listings/latest")
            .header("X-CMC_PRO_API_KEY", &self.api_key)
            .query(&[("start", "1"), ("limit", "1000"), ("convert", "USD")])
            .send()
            .await?;

        let cmc_response: CmcResponse = response.json().await?;
        Ok(cmc_response.data)
    }

    pub async fn get_quote(&self, symbol: &str) -> Result<CmcQuote> {
        let response = self
            .client
            .get("https://pro-api.coinmarketcap.com/v2/cryptocurrency/quotes/latest")
            .header("X-CMC_PRO_API_KEY", &self.api_key)
            .query(&[("symbol", symbol), ("convert", "USD")])
            .send()
            .await?;

        let quote_response: CmcQuoteResponse = response.json().await?;
        let listings = quote_response
            .data
            .get(symbol)
            .ok_or_else(|| anyhow!("No quote data for symbol {}", symbol))?;
        let listing = listings
            .first()
            .ok_or_else(|| anyhow!("No quote data available for symbol {}", symbol))?;
        Ok(listing.quote.usd.clone())
    }
}

pub async fn update_assets(pool: &PgPool) -> Result<()> {
    let service = CmcService::new();
    let listings = service.fetch_cmc_listings().await?;

    for listing in listings {
        let cmc_id = listing.id.to_string();

        sqlx::query!(
            r#"
            INSERT INTO assets (symbol, name, cmc_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (cmc_id) DO UPDATE
            SET symbol = EXCLUDED.symbol, name = EXCLUDED.name
            "#,
            listing.symbol,
            listing.name,
            cmc_id,
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}
