use crate::dto::asset::{AssetDto, CreateAssetDto};
use crate::repository::asset::AssetRepository;
use actix_web::web;
use anyhow::Result;
use sqlx::PgPool;

// Service for managing assets
#[derive(Clone)]
pub struct AssetService {
    pool: web::Data<PgPool>,
}

impl AssetService {
    // Creates a new instance of AssetService
    pub fn new(pool: web::Data<PgPool>) -> Self {
        Self { pool }
    }

    // Creates a new asset
    pub async fn create(&self, asset: CreateAssetDto) -> Result<AssetDto> {
        let repo = AssetRepository::new(self.pool.as_ref());
        let record = repo
            .create(
                asset.symbol,
                asset.name,
                asset.cmc_id,
                asset.decimals,
                asset.rank,
            )
            .await?;

        // Map the database record to DTO
        Ok(AssetDto {
            id: record.id,
            symbol: record.symbol,
            name: record.name,
            cmc_id: record.cmc_id,
            decimals: record.decimals,
            rank: record.rank,
            created_at: record.created_at.to_string(),
        })
    }

    // Retrieves all assets
    pub async fn get_all(&self) -> Result<Vec<AssetDto>> {
        let repo = AssetRepository::new(self.pool.as_ref());
        let assets = repo.get_all().await?;

        // Map database records to DTOs
        let response = assets
            .into_iter()
            .map(|record| AssetDto {
                id: record.id,
                symbol: record.symbol,
                name: record.name,
                cmc_id: record.cmc_id,
                decimals: record.decimals,
                rank: record.rank,
                created_at: record.created_at.to_string(),
            })
            .collect();

        Ok(response)
    }
}
