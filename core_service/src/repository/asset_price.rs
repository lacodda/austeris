use crate::models::cmc::CmcQuote;
use crate::services::redis::RedisService;
use anyhow::Result;
use sqlx::types::time::PrimitiveDateTime;
use sqlx::{query_builder::QueryBuilder, PgPool, Postgres};

// Repository for asset price-related database operations
pub struct AssetPriceRepository<'a> {
    pool: &'a PgPool,
    redis: RedisService,
}

impl<'a> AssetPriceRepository<'a> {
    // Creates a new instance of AssetPriceRepository
    pub fn new(pool: &'a PgPool, redis: RedisService) -> Self {
        Self { pool, redis }
    }

    // Saves asset prices into the asset_prices table and caches them in Redis
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
                        RETURNING timestamp
                        "#,
                        asset_id,
                        price_usd
                    )
                    .fetch_one(self.pool)
                    .await?;

                    inserted_count += 1;

                    // Cache the price in Redis with the timestamp returned from DB
                    let timestamp = result.timestamp.to_string();
                    self.redis
                        .save_price(asset_id, price_usd, timestamp)
                        .await?;
                }
            }
        }

        Ok(inserted_count)
    }

    // Gets the latest prices for all assets from asset_prices
    pub async fn get_latest_prices(&self) -> Result<Vec<(i32, f64, PrimitiveDateTime)>> {
        let prices = sqlx::query!(
            r#"
            SELECT a.cmc_id, ap.price_usd, ap.timestamp
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

    // Gets the latest prices with asset details, optionally filtered by asset_ids, sorted by rank
    pub async fn get_latest_prices_with_assets(
        &self,
        asset_ids: Option<Vec<i32>>,
    ) -> Result<Vec<(i32, String, String, f64, PrimitiveDateTime)>> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT 
                a.cmc_id, 
                a.symbol, 
                a.name, 
                ap.price_usd, 
                ap.timestamp
            FROM asset_prices ap
            JOIN assets a ON a.id = ap.asset_id
            WHERE ap.timestamp = (
                SELECT MAX(timestamp)
                FROM asset_prices
                WHERE asset_id = ap.asset_id
            )
            "#,
        );

        // Add filter by asset_ids if provided
        if let Some(ids) = asset_ids {
            if !ids.is_empty() {
                query_builder.push(" AND a.id = ANY(");
                query_builder.push_bind(ids);
                query_builder.push(")");
            }
        }

        // Add sorting by rank
        query_builder.push(" ORDER BY a.rank ASC");

        let prices = query_builder
            .build_query_as::<(i32, String, String, f64, PrimitiveDateTime)>()
            .fetch_all(self.pool)
            .await?;

        Ok(prices)
    }

    // Gets historical prices
    pub async fn get_price_history(
        &self,
        asset_ids: Option<Vec<i32>>,
        start_date: PrimitiveDateTime,
        end_date: Option<PrimitiveDateTime>,
    ) -> Result<Vec<(i32, String, f64, PrimitiveDateTime)>> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT 
                a.cmc_id, 
                a.symbol, 
                ap.price_usd, 
                ap.timestamp
            FROM asset_prices ap
            JOIN assets a ON a.id = ap.asset_id
            WHERE ap.timestamp >= 
            "#,
        );
        query_builder.push_bind(start_date);

        if let Some(end) = end_date {
            query_builder.push(" AND ap.timestamp <= ");
            query_builder.push_bind(end);
        }

        if let Some(ids) = asset_ids {
            if !ids.is_empty() {
                query_builder.push(" AND a.id = ANY(");
                query_builder.push_bind(ids);
                query_builder.push(")");
            }
        }

        query_builder.push(" ORDER BY ap.timestamp ASC");

        let history = query_builder
            .build_query_as::<(i32, String, f64, PrimitiveDateTime)>()
            .fetch_all(self.pool)
            .await?;

        Ok(history)
    }
}
