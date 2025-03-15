use crate::{dto::asset::CreateAssetDto, models::asset::AssetDb};
use anyhow::Result;
use sqlx::PgPool;

// Repository for asset-related database operations
pub struct AssetRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> AssetRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Creates a new asset in the database
    pub async fn create(
        &self,
        symbol: String,
        name: String,
        cmc_id: i32,
        decimals: Option<i32>,
        rank: Option<i32>,
    ) -> Result<AssetDb> {
        let record = sqlx::query_as!(
            AssetDb,
            r#"
            INSERT INTO assets (symbol, name, cmc_id, decimals, rank)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, symbol, name, cmc_id, decimals, rank, created_at
            "#,
            symbol,
            name,
            cmc_id,
            decimals,
            rank
        )
        .fetch_one(self.pool)
        .await?;
        Ok(record)
    }

    // Retrieves all assets from the database
    pub async fn get_all(&self) -> Result<Vec<AssetDb>> {
        let assets = sqlx::query_as!(
            AssetDb,
            r#"
            SELECT id, symbol, name, cmc_id, decimals, rank, created_at
            FROM assets
            ORDER BY id ASC
            "#,
        )
        .fetch_all(self.pool)
        .await?;
        Ok(assets)
    }

    // Checks if an asset with the given ID exists
    pub async fn exists(&self, id: i32) -> Result<bool> {
        let exists = sqlx::query!("SELECT EXISTS(SELECT 1 FROM assets WHERE id = $1)", id)
            .fetch_one(self.pool)
            .await?
            .exists
            .unwrap_or(false);
        Ok(exists)
    }

    // Updates assets in the database
    pub async fn update_assets(&self, listings: Vec<CreateAssetDto>) -> Result<usize> {
        let mut updated_count = 0;
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
                listing.cmc_id,
                listing.rank.unwrap_or(0),
            )
            .execute(self.pool)
            .await?;
            updated_count += result.rows_affected() as usize;
        }
        Ok(updated_count)
    }

    // Fetches all cmc_ids from the assets table
    pub async fn get_all_cmc_ids(&self) -> Result<Vec<i32>> {
        let cmc_ids = sqlx::query_scalar!("SELECT cmc_id FROM assets")
            .fetch_all(self.pool)
            .await?;
        Ok(cmc_ids)
    }

    // Fetches cmc_id by asset_id
    pub async fn get_cmc_id_by_asset_id(&self, asset_id: i32) -> Result<Option<i32>> {
        let cmc_id = sqlx::query_scalar!("SELECT cmc_id FROM assets WHERE id = $1", asset_id)
            .fetch_optional(self.pool)
            .await?;
        Ok(cmc_id)
    }
}
