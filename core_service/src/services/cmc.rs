use crate::models::cmc::{CmcListing, CmcResponse};
use reqwest::Error;
use sqlx::PgPool;
use std::env;

pub async fn fetch_cmc_listings() -> Result<Vec<CmcListing>, Error> {
    let api_key = env::var("COINMARKETCAP_API_KEY").expect("COINMARKETCAP_API_KEY must be set");

    let client = reqwest::Client::new();
    let response = client
        .get("https://pro-api.coinmarketcap.com/v1/cryptocurrency/listings/latest")
        .header("X-CMC_PRO_API_KEY", api_key)
        .query(&[("start", "1"), ("limit", "1000"), ("convert", "USD")])
        .send()
        .await?;

    let cmc_response: CmcResponse = response.json().await?;
    Ok(cmc_response.data)
}

pub async fn update_assets(pool: &PgPool) -> Result<(), sqlx::Error> {
    let listings = fetch_cmc_listings().await.map_err(|e| {
        sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to fetch listings: {}", e),
        ))
    })?;

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
