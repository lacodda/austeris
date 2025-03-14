use crate::dto::asset::{
    AssetDto, AssetPriceHistoryDto, AssetPriceWithDetailsDto, CreateAssetDto, UpdateAssetsResponse,
};
use crate::error::AppError;
use crate::models::asset::{HistoryQueryParams, PriceQueryParams};
use crate::repository::asset::AssetRepository;
use crate::repository::asset_price::AssetPriceRepository;
use crate::services::cmc::CmcService;
use crate::services::redis::RedisService;
use crate::utils::datetime::{format_iso8601, parse_iso8601};
use actix_web::web;
use anyhow::{Ok, Result};
use sqlx::PgPool;

// Service for managing assets
#[derive(Clone)]
pub struct AssetService {
    pool: web::Data<PgPool>,
    cmc_service: web::Data<CmcService>,
    redis_service: web::Data<RedisService>,
}

impl AssetService {
    // Creates a new instance of AssetService
    pub fn new(
        pool: web::Data<PgPool>,
        cmc_service: web::Data<CmcService>,
        redis_service: web::Data<RedisService>,
    ) -> Self {
        Self {
            pool,
            cmc_service,
            redis_service,
        }
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
        Ok(record.into())
    }

    // Retrieves all assets
    pub async fn get_all(&self) -> Result<Vec<AssetDto>> {
        let repo = AssetRepository::new(self.pool.as_ref());
        let assets = repo.get_all().await?;
        Ok(assets.into_iter().map(AssetDto::from).collect())
    }

    // Updates the assets table with data from CoinMarketCap and returns the number of updated assets
    pub async fn update(&self) -> Result<UpdateAssetsResponse> {
        let listings = self.cmc_service.fetch_cmc_listings().await?;
        let repo = AssetRepository::new(self.pool.as_ref());
        let updated_count = repo
            .update_assets(
                listings
                    .into_iter()
                    .map(|l| CreateAssetDto {
                        symbol: l.symbol,
                        name: l.name,
                        cmc_id: l.id,
                        decimals: None,
                        rank: Some(l.cmc_rank),
                    })
                    .collect(),
            )
            .await?;
        let response = UpdateAssetsResponse {
            updated_count,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        Ok(response)
    }

    // Get latest asset prices with details
    pub async fn get_prices(
        &self,
        query: PriceQueryParams,
    ) -> Result<Vec<AssetPriceWithDetailsDto>> {
        let price_repo =
            AssetPriceRepository::new(self.pool.as_ref(), self.redis_service.as_ref().clone());
        let asset_ids = query.asset_ids.as_ref().map(|ids| {
            ids.split(',')
                .filter_map(|id| id.trim().parse::<i32>().ok())
                .collect::<Vec<i32>>()
        });

        let prices = price_repo.get_latest_prices_with_assets(asset_ids).await?;
        let response = prices
            .into_iter()
            .map(
                |(cmc_id, symbol, name, price_usd, timestamp)| AssetPriceWithDetailsDto {
                    cmc_id,
                    symbol,
                    name,
                    price_usd,
                    timestamp: format_iso8601(timestamp),
                },
            )
            .collect::<Vec<_>>();
        Ok(response)
    }

    // Get history asset prices with details
    pub async fn get_price_history(
        &self,
        query: HistoryQueryParams,
    ) -> Result<Vec<AssetPriceHistoryDto>> {
        let price_repo =
            AssetPriceRepository::new(self.pool.as_ref(), self.redis_service.as_ref().clone());
        let asset_ids = query.asset_ids.as_ref().map(|ids| {
            ids.split(',')
                .filter_map(|id| id.trim().parse::<i32>().ok())
                .collect::<Vec<i32>>()
        });

        let start_date = parse_iso8601(&query.start_date).map_err(|e| AppError::bad_request(e))?;
        let end_date = query
            .end_date
            .as_ref()
            .map(|end| parse_iso8601(end).map_err(|e| AppError::bad_request(e)))
            .transpose()?;

        let history = price_repo
            .get_price_history(asset_ids, start_date, end_date)
            .await
            .map_err(AppError::internal)?;

        let response = history
            .into_iter()
            .map(
                |(cmc_id, symbol, price_usd, timestamp)| AssetPriceHistoryDto {
                    cmc_id,
                    symbol,
                    price_usd,
                    timestamp: format_iso8601(timestamp),
                },
            )
            .collect::<Vec<_>>();
        Ok(response)
    }
}
