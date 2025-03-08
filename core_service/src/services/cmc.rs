use crate::models::cmc::{CmcListing, CmcQuote, CmcQuoteResponse, CmcResponse};
use anyhow::{anyhow, Result};
use sqlx::PgPool;
use std::env;

// Represents a service for interacting with the CoinMarketCap API
#[derive(Clone)]
pub struct CmcService {
    client: reqwest::Client,
    api_key: String,
}

impl CmcService {
    // Creates a new instance of CmcService with API key from environment
    pub fn new() -> Self {
        let api_key = env::var("COINMARKETCAP_API_KEY").expect("COINMARKETCAP_API_KEY must be set");
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }

    // Fetches the latest cryptocurrency listings from CoinMarketCap
    pub async fn fetch_cmc_listings(&self) -> Result<Vec<CmcListing>> {
        let response = self
            .client
            .get("https://pro-api.coinmarketcap.com/v1/cryptocurrency/listings/latest")
            .header("X-CMC_PRO_API_KEY", &self.api_key)
            .query(&[("start", "1"), ("limit", "1000"), ("convert", "USD")])
            .send()
            .await?;

        // Parse the response into CmcResponse structure
        let cmc_response: CmcResponse = response.json().await?;
        Ok(cmc_response.data)
    }

    // Fetches the latest quote for a specific cryptocurrency symbol
    pub async fn get_quote(&self, symbol: &str) -> Result<CmcQuote> {
        let response = self
            .client
            .get("https://pro-api.coinmarketcap.com/v2/cryptocurrency/quotes/latest")
            .header("X-CMC_PRO_API_KEY", &self.api_key)
            .query(&[("symbol", symbol), ("convert", "USD")])
            .send()
            .await?;

        // Parse the response into CmcQuoteResponse structure
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

    // Fetches quotes for all assets in the database using their cmc_id
    pub async fn fetch_quotes_for_assets(&self, pool: &PgPool) -> Result<Vec<(i32, CmcQuote)>> {
        // Fetch all cmc_ids from the assets table
        let cmc_ids: Vec<i32> = sqlx::query_scalar!("SELECT cmc_id FROM assets")
            .fetch_all(pool)
            .await?;

        if cmc_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Batch cmc_ids into chunks of 100 (API limit)
        const BATCH_SIZE: usize = 100;
        let mut quotes = Vec::new();
        let usd = String::from("USD");

        for chunk in cmc_ids.chunks(BATCH_SIZE) {
            let ids_str = chunk
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<String>>()
                .join(",");
            let response = self
                .client
                .get("https://pro-api.coinmarketcap.com/v2/cryptocurrency/quotes/latest")
                .header("X-CMC_PRO_API_KEY", &self.api_key)
                .query(&[("id", &ids_str), ("convert", &usd)])
                .send()
                .await?;

            let quote_response: CmcQuoteResponse = response.json().await?;
            for (cmc_id_str, listings) in quote_response.data {
                let cmc_id: i32 = cmc_id_str.parse().expect("CMC ID should be an integer");
                if let Some(listing) = listings.first() {
                    quotes.push((cmc_id, listing.quote.usd.clone()));
                }
            }
        }

        Ok(quotes)
    }
}

// Updates the assets table with data from CoinMarketCap and returns the number of updated assets
pub async fn update_assets(pool: &PgPool) -> Result<usize> {
    let service = CmcService::new();
    let listings = service.fetch_cmc_listings().await?;
    let mut updated_count = 0;

    // Iterate over listings and upsert into the assets table
    for listing in listings {
        let result = sqlx::query!(
            r#"
            INSERT INTO assets (symbol, name, cmc_id, rank)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (cmc_id) DO UPDATE
            SET symbol = EXCLUDED.symbol, name = EXCLUDED.name, rank = EXCLUDED.rank
            "#,
            listing.symbol,
            listing.name,
            listing.id,
            listing.cmc_rank,
        )
        .execute(pool)
        .await?;
        updated_count += result.rows_affected() as usize;
    }

    Ok(updated_count)
}
