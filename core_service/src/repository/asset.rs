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
}
