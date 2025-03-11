use crate::models::asset::AssetDb;
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

    // Checks if an asset with the given ID exists
    pub async fn exists(&self, asset_id: i32) -> Result<bool> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM assets WHERE id = $1")
            .bind(asset_id)
            .fetch_one(self.pool)
            .await?;
        Ok(count > 0)
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
}
