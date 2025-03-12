use crate::models::cmc::{CmcListing, CmcQuote, CmcQuoteResponse, CmcResponse};
use anyhow::Result;
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
            for (cmc_id_str, listing) in quote_response.data {
                let cmc_id: i32 = cmc_id_str.parse().expect("CMC ID should be an integer");
                quotes.push((cmc_id, listing.quote.usd.clone()));
            }
        }

        Ok(quotes)
    }
}
