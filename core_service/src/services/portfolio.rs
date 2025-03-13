use crate::dto::snapshot::SnapshotAssetDto;
use crate::repository::asset_price::AssetPriceRepository;
use crate::repository::transaction::TransactionRepository;
use crate::services::cmc::CmcService;
use crate::services::redis::RedisService;
use crate::utils::datetime::parse_iso8601;
use actix_web::web;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::PgPool;
use std::collections::HashMap;
use time::OffsetDateTime;

// PortfolioService handles portfolio-related calculations
#[derive(Clone)]
pub struct PortfolioService {
    pool: web::Data<PgPool>,
    cmc_service: web::Data<CmcService>,
    redis_service: web::Data<RedisService>,
}

impl PortfolioService {
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

    // Calculates current asset holdings from all transactions
    pub async fn get_current_assets(&self) -> Result<HashMap<String, (f64, i32)>> {
        let transaction_repo = TransactionRepository::new(self.pool.as_ref());
        let transactions = transaction_repo.get_all_transactions().await?;
        let mut asset_amounts: HashMap<String, (f64, i32)> = HashMap::new();

        for record in transactions {
            let (amount, cmc_id) = asset_amounts
                .entry(record.asset.clone())
                .or_insert((0.0, 0));
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
        let price_repo =
            AssetPriceRepository::new(self.pool.as_ref(), self.redis_service.as_ref().clone());
        let mut total_value = 0.0;

        // Identify assets needing fresh prices (missing or older than 1 hour)
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let mut cmc_ids_to_fetch: Vec<i32> = Vec::new();
        let mut price_map: HashMap<i32, (f64, PrimitiveDateTime)> = HashMap::new();

        // Step 1: Try to get prices from Redis first
        for (_symbol, (amount, cmc_id)) in &asset_amounts {
            if *amount > 0.0 {
                match self.redis_service.get_price(*cmc_id).await? {
                    Some(cached) => {
                        let timestamp = parse_iso8601(&cached.timestamp)?;
                        let timestamp_offset = timestamp.assume_utc();
                        let timestamp_utc: DateTime<Utc> = DateTime::from_timestamp(
                            timestamp_offset.unix_timestamp(),
                            timestamp_offset.nanosecond(),
                        )
                        .unwrap_or_else(|| Utc::now());

                        if timestamp_utc >= one_hour_ago {
                            total_value += amount * cached.price_usd;
                            price_map.insert(*cmc_id, (cached.price_usd, timestamp));
                        } else {
                            cmc_ids_to_fetch.push(*cmc_id);
                        }
                    }
                    None => {
                        cmc_ids_to_fetch.push(*cmc_id);
                    }
                }
            }
        }

        // Step 2: Fetch missing or outdated prices from DB or CMC
        if !cmc_ids_to_fetch.is_empty() {
            // Try DB first
            let latest_prices = price_repo.get_latest_prices().await?;
            let db_prices: HashMap<i32, (f64, PrimitiveDateTime)> = latest_prices
                .into_iter()
                .map(|(cmc_id, price, timestamp)| (cmc_id, (price, timestamp)))
                .collect();

            let mut still_missing: Vec<i32> = Vec::new();
            for cmc_id in &cmc_ids_to_fetch {
                if let Some((price, timestamp)) = db_prices.get(cmc_id) {
                    let timestamp_offset = timestamp.assume_utc();
                    let timestamp_utc: DateTime<Utc> = DateTime::from_timestamp(
                        timestamp_offset.unix_timestamp(),
                        timestamp_offset.nanosecond(),
                    )
                    .unwrap_or_else(|| Utc::now());

                    if timestamp_utc >= one_hour_ago {
                        if let Some((_, (amount, _))) =
                            asset_amounts.iter().find(|(_, (_, id))| id == cmc_id)
                        {
                            total_value += amount * price; // amount is &f64, price is &f64
                            price_map.insert(*cmc_id, (*price, *timestamp));
                        }
                    } else {
                        still_missing.push(*cmc_id);
                    }
                } else {
                    still_missing.push(*cmc_id);
                }
            }

            // Fetch from CMC if still missing or outdated
            if !still_missing.is_empty() {
                let fresh_quotes = self
                    .cmc_service
                    .fetch_quotes_for_assets(self.pool.as_ref())
                    .await?;
                price_repo.save_prices(fresh_quotes.clone()).await?;

                let now_offset = OffsetDateTime::now_utc();
                let now_pdt = PrimitiveDateTime::new(now_offset.date(), now_offset.time());
                for (cmc_id, quote) in fresh_quotes {
                    if let Some(price) = quote.price {
                        price_map.insert(cmc_id, (price, now_pdt));
                        if let Some((_, (amount, _))) =
                            asset_amounts.iter().find(|(_, (_, id))| id == &cmc_id)
                        {
                            total_value += amount * price; // amount is &f64, price is f64
                        }
                    } else {
                        log::warn!("No price available for cmc_id {} after CMC fetch", cmc_id);
                    }
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
