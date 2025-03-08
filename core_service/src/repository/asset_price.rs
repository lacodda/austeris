use crate::models::cmc::CmcQuote;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

// Repository for asset price-related database operations
pub struct AssetPriceRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> AssetPriceRepository<'a> {
    // Creates a new instance of AssetPriceRepository
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Saves asset prices into the asset_prices table
    pub async fn save_prices(&self, prices: Vec<(i32, CmcQuote)>) -> Result<usize> {
        let mut inserted_count = 0;

        for (cmc_id, quote) in prices {
            if let Some(price_usd) = quote.price {
                // Fetch asset_id by cmc_id
                let asset_id =
                    sqlx::query_scalar!("SELECT id FROM assets WHERE cmc_id = $1", cmc_id)
                        .fetch_optional(self.pool)
                        .await?;

                if let Some(asset_id) = asset_id {
                    let result = sqlx::query!(
                        r#"
                        INSERT INTO asset_prices (asset_id, price_usd)
                        VALUES ($1, $2)
                        ON CONFLICT (asset_id, timestamp) DO NOTHING
                        "#,
                        asset_id,
                        price_usd
                    )
                    .execute(self.pool)
                    .await?;

                    inserted_count += result.rows_affected() as usize;
                }
            }
        }

        Ok(inserted_count)
    }

    // Gets the latest prices for all assets from asset_prices
    pub async fn get_latest_prices(&self) -> Result<Vec<(i32, f64, DateTime<Utc>)>> {
        let prices = sqlx::query!(
            r#"
            SELECT a.cmc_id, ap.price_usd, ap.timestamp as "timestamp: DateTime<Utc>"
            FROM asset_prices ap
            JOIN assets a ON a.id = ap.asset_id
            WHERE ap.timestamp = (
                SELECT MAX(timestamp)
                FROM asset_prices
                WHERE asset_id = ap.asset_id
            )
            "#,
        )
        .fetch_all(self.pool)
        .await?
        .into_iter()
        .map(|record| (record.cmc_id, record.price_usd, record.timestamp))
        .collect();

        Ok(prices)
    }

    // Gets the latest prices with asset details from asset_prices and assets
    pub async fn get_latest_prices_with_assets(
        &self,
    ) -> Result<Vec<(i32, String, String, f64, DateTime<Utc>)>> {
        let prices = sqlx::query!(
            r#"
            SELECT 
                a.cmc_id, 
                a.symbol, 
                a.name, 
                ap.price_usd, 
                ap.timestamp as "timestamp: DateTime<Utc>"
            FROM asset_prices ap
            JOIN assets a ON a.id = ap.asset_id
            WHERE ap.timestamp = (
                SELECT MAX(timestamp)
                FROM asset_prices
                WHERE asset_id = ap.asset_id
            )
            ORDER BY a.rank ASC
            "#,
        )
        .fetch_all(self.pool)
        .await?
        .into_iter()
        .map(|record| {
            (
                record.cmc_id,
                record.symbol,
                record.name,
                record.price_usd,
                record.timestamp,
            )
        })
        .collect();

        Ok(prices)
    }
}
