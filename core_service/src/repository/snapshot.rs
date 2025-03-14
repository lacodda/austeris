use crate::dto::snapshot::SnapshotAssetDto;
use crate::error::AppError;
use crate::models::snapshot::SnapshotDb;
use anyhow::Result;
use sqlx::PgPool;

// Repository for snapshot-related database operations
pub struct SnapshotRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> SnapshotRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Creates a new snapshot in the database
    pub async fn create(&self, assets: Vec<SnapshotAssetDto>) -> Result<SnapshotDb, AppError> {
        let record = sqlx::query_as::<_, SnapshotDb>(
            r#"
            INSERT INTO portfolio_snapshots (assets)
            VALUES ($1)
            RETURNING id, created_at, assets
            "#,
        )
        .bind(sqlx::types::Json(&assets))
        .fetch_one(self.pool)
        .await
        .map_err(AppError::internal)?;
        Ok(record)
    }

    // Retrieves all snapshots from the database
    pub async fn get_all(&self) -> Result<Vec<SnapshotDb>, AppError> {
        let snapshots = sqlx::query_as::<_, SnapshotDb>(
            r#"
            SELECT id, created_at, assets
            FROM portfolio_snapshots
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(AppError::internal)?;
        Ok(snapshots)
    }
}
