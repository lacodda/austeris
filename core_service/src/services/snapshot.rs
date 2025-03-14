use crate::dto::snapshot::{SnapshotDiffDto, SnapshotDto};
use crate::error::AppError;
use crate::repository::snapshot::SnapshotRepository;
use crate::services::portfolio::PortfolioService;
use actix_web::web;
use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashMap;

// Service for managing portfolio snapshots
#[derive(Clone)]
pub struct SnapshotService {
    pool: web::Data<PgPool>,
    portfolio_service: web::Data<PortfolioService>,
}

impl SnapshotService {
    // Creates a new instance of SnapshotService
    pub fn new(pool: web::Data<PgPool>, portfolio_service: web::Data<PortfolioService>) -> Self {
        Self {
            pool,
            portfolio_service,
        }
    }

    // Creates a new portfolio snapshot
    pub async fn create(&self) -> Result<SnapshotDto, AppError> {
        let snapshot_assets = self
            .portfolio_service
            .get_current_snapshot()
            .await
            .map_err(AppError::internal)?;
        let repo = SnapshotRepository::new(self.pool.as_ref());
        let record = repo.create(snapshot_assets).await?;
        Ok(record.into())
    }

    // Retrieves all snapshots with differences from current state
    pub async fn get_all(&self) -> Result<Vec<SnapshotDto>, AppError> {
        let repo = SnapshotRepository::new(self.pool.as_ref());
        let snapshots = repo.get_all().await?;

        let current_assets = self
            .portfolio_service
            .get_current_assets()
            .await
            .map_err(AppError::internal)?;

        // Map snapshots to DTOs with calculated differences
        let response: Vec<SnapshotDto> = snapshots
            .into_iter()
            .map(|record| {
                let mut dto: SnapshotDto = record.into();
                let mut diff_map: HashMap<String, SnapshotDiffDto> = HashMap::new();

                // Calculate differences between snapshot and current state
                for asset in &dto.assets {
                    let current = current_assets
                        .get(&asset.symbol)
                        .map(|(amt, _)| *amt)
                        .unwrap_or(0.0);
                    let diff = current - asset.amount;
                    if diff != 0.0 {
                        diff_map.insert(
                            asset.symbol.clone(),
                            SnapshotDiffDto {
                                symbol: asset.symbol.clone(),
                                amount_diff: diff,
                                cmc_id: asset.cmc_id,
                            },
                        );
                    }
                }

                // Include assets present now but not in the snapshot
                for (symbol, (current_amount, cmc_id)) in &current_assets {
                    if *current_amount > 0.0 && !dto.assets.iter().any(|a| &a.symbol == symbol) {
                        diff_map.insert(
                            symbol.clone(),
                            SnapshotDiffDto {
                                symbol: symbol.clone(),
                                amount_diff: *current_amount,
                                cmc_id: *cmc_id,
                            },
                        );
                    }
                }

                dto.diff = Some(diff_map.into_values().collect());
                dto
            })
            .collect();

        Ok(response)
    }
}
