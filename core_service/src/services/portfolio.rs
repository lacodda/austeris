use crate::dto::snapshot::SnapshotAssetDto;
use crate::repository::transaction::TransactionRepository;
use crate::services::cmc::CmcService;
use actix_web::web;
use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashMap;

// PortfolioService handles portfolio-related calculations
#[derive(Clone)]
pub struct PortfolioService {
    pool: web::Data<PgPool>,
    cmc_service: web::Data<CmcService>,
}

impl PortfolioService {
    // Creates a new instance of PortfolioService
    pub fn new(pool: web::Data<PgPool>, cmc_service: web::Data<CmcService>) -> Self {
        Self { pool, cmc_service }
    }

    // Calculates current asset holdings from all transactions
    pub async fn get_current_assets(&self) -> Result<HashMap<String, (f64, i32)>> {
        let transaction_repo = TransactionRepository::new(self.pool.as_ref());
        let transactions = transaction_repo.get_all_transactions().await?;
        let mut asset_amounts: HashMap<String, (f64, i32)> = HashMap::new();

        for record in transactions {
            let (amount, cmc_id) = asset_amounts
                .entry(record.asset.clone())
                .or_insert((0.0, 0)); // Default cmc_id as 0
            if *cmc_id == 0 {
                // Check if cmc_id needs to be fetched
                let asset_cmc_id = sqlx::query_scalar!(
                    "SELECT cmc_id FROM assets WHERE symbol = $1",
                    record.asset
                )
                .fetch_one(self.pool.as_ref())
                .await?;
                *cmc_id = asset_cmc_id;
            }
            if record.transaction_type == "BUY" {
                *amount += record.amount;
            } else if record.transaction_type == "SELL" {
                *amount -= record.amount;
            }
        }

        Ok(asset_amounts)
    }

    // Calculates the total portfolio value in USD
    pub async fn get_portfolio_value(&self) -> Result<f64> {
        let asset_amounts = self.get_current_assets().await?;
        let mut total_value = 0.0;

        for (symbol, (amount, _)) in asset_amounts {
            if amount > 0.0 {
                let quote = self.cmc_service.get_quote(&symbol).await?;
                if let Some(price) = quote.price {
                    total_value += amount * price;
                }
            }
        }

        Ok(total_value)
    }

    // Generates a snapshot of current assets
    pub async fn get_current_snapshot(&self) -> Result<Vec<SnapshotAssetDto>> {
        let asset_amounts = self.get_current_assets().await?;
        let snapshot_assets: Vec<SnapshotAssetDto> = asset_amounts
            .into_iter()
            .filter(|(_, (amount, _))| *amount > 0.0)
            .map(|(symbol, (amount, cmc_id))| SnapshotAssetDto {
                symbol,
                amount,
                cmc_id,
            })
            .collect();

        Ok(snapshot_assets)
    }
}
