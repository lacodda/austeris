use crate::dto::snapshot::{SnapshotAssetDto, SnapshotDiffDto, SnapshotDto};
use crate::models::snapshot::SnapshotDb;
use crate::repository::transaction::TransactionRepository;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use log::error;
use sqlx::PgPool;
use std::collections::HashMap;

// Configures routes for the /snapshots scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/snapshots")
            .route("", web::post().to(create_snapshot))
            .route("", web::get().to(get_snapshots)),
    );
}

// Handles POST /snapshots to create a new portfolio snapshot
#[utoipa::path(
    post,
    path = "/snapshots",
    responses(
        (status = 200, description = "Snapshot created successfully", body = SnapshotDto, example = json!({"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": "1"}, {"symbol": "ETH", "amount": 10.0, "cmc_id": "1027"}]})),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to save snapshot to database"))
    )
)]
async fn create_snapshot(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<SnapshotDto> = (|| async {
        // Use the repository to fetch all transactions
        let repo = TransactionRepository::new(pool.get_ref());
        let transactions = repo.get_all_transactions().await?;

        // Calculate current asset holdings from transactions
        let mut asset_amounts: HashMap<String, (f64, String)> = HashMap::new();
        for record in transactions {
            let (amount, cmc_id) = asset_amounts
                .entry(record.asset.clone())
                .or_insert((0.0, String::new()));
            if *cmc_id == String::new() {
                let asset_cmc_id =
                    sqlx::query!("SELECT cmc_id FROM assets WHERE symbol = $1", record.asset)
                        .fetch_one(pool.get_ref())
                        .await?
                        .cmc_id;
                *cmc_id = asset_cmc_id;
            }
            if record.transaction_type == "BUY" {
                *amount += record.amount;
            } else if record.transaction_type == "SELL" {
                *amount -= record.amount;
            }
        }

        // Filter out assets with zero or negative amounts and map to DTO
        let snapshot_assets: Vec<SnapshotAssetDto> = asset_amounts
            .into_iter()
            .filter(|(_, (amount, _))| *amount > 0.0)
            .map(|(symbol, (amount, cmc_id))| SnapshotAssetDto {
                symbol,
                amount,
                cmc_id,
            })
            .collect();

        // Save the snapshot to the database
        let record = sqlx::query_as::<_, SnapshotDb>(
            r#"
            INSERT INTO portfolio_snapshots (assets)
            VALUES ($1)
            RETURNING id, created_at, assets
            "#,
        )
        .bind(sqlx::types::Json(&snapshot_assets))
        .fetch_one(pool.get_ref())
        .await?;

        Ok(SnapshotDto {
            id: record.id,
            created_at: record.created_at.to_string(),
            assets: snapshot_assets,
            diff: None,
        })
    })()
    .await;

    match result {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(e) => {
            error!("Failed to create snapshot: {}", e);
            HttpResponse::InternalServerError().json(e.to_string())
        }
    }
}

// Handles GET /snapshots to retrieve all snapshots with differences
#[utoipa::path(
    get,
    path = "/snapshots",
    responses(
        (status = 200, description = "Successfully retrieved list of snapshots with differences", body = Vec<SnapshotDto>, example = json!([{"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": "1"}, {"symbol": "ETH", "amount": 10.0, "cmc_id": "1027"}], "diff": [{"symbol": "BTC", "amount_diff": -0.5, "cmc_id": "1"}, {"symbol": "ETH", "amount_diff": 2.0, "cmc_id": "1027"}]}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to fetch snapshots from database"))
    )
)]
async fn get_snapshots(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<Vec<SnapshotDto>> = (|| async {
        // Fetch all snapshots from the database
        let snapshots = sqlx::query_as::<_, SnapshotDb>(
            r#"
            SELECT id, created_at, assets
            FROM portfolio_snapshots
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(pool.get_ref())
        .await?;

        // Use the repository to fetch all transactions
        let repo = TransactionRepository::new(pool.get_ref());
        let transactions = repo.get_all_transactions().await?;

        // Calculate current asset holdings from transactions
        let mut current_assets: HashMap<String, (f64, String)> = HashMap::new();
        for record in transactions {
            let (amount, cmc_id) = current_assets
                .entry(record.asset.clone())
                .or_insert((0.0, String::new()));
            if *cmc_id == String::new() {
                let asset_cmc_id =
                    sqlx::query!("SELECT cmc_id FROM assets WHERE symbol = $1", record.asset)
                        .fetch_one(pool.get_ref())
                        .await?
                        .cmc_id;
                *cmc_id = asset_cmc_id;
            }
            if record.transaction_type == "BUY" {
                *amount += record.amount;
            } else if record.transaction_type == "SELL" {
                *amount -= record.amount;
            }
        }

        // Map snapshots to DTO with calculated differences
        let snapshots_with_diff: Vec<SnapshotDto> = snapshots
            .into_iter()
            .map(|record| {
                let snapshot_assets: Vec<SnapshotAssetDto> = record
                    .assets
                    .0
                    .clone()
                    .into_iter()
                    .map(|asset| SnapshotAssetDto {
                        symbol: asset.symbol,
                        amount: asset.amount,
                        cmc_id: asset.cmc_id,
                    })
                    .collect();
                let mut diff_map: HashMap<String, SnapshotDiffDto> = HashMap::new();

                // Calculate differences between snapshot and current state
                for asset in &snapshot_assets {
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
                                cmc_id: asset.cmc_id.clone(),
                            },
                        );
                    }
                }

                // Include assets present now but not in the snapshot
                for (symbol, (current_amount, cmc_id)) in &current_assets {
                    if *current_amount > 0.0 && !snapshot_assets.iter().any(|a| &a.symbol == symbol)
                    {
                        diff_map.insert(
                            symbol.clone(),
                            SnapshotDiffDto {
                                symbol: symbol.clone(),
                                amount_diff: *current_amount,
                                cmc_id: cmc_id.clone(),
                            },
                        );
                    }
                }

                SnapshotDto {
                    id: record.id,
                    created_at: record.created_at.to_string(),
                    assets: snapshot_assets,
                    diff: Some(diff_map.into_values().collect()),
                }
            })
            .collect();

        Ok(snapshots_with_diff)
    })()
    .await;

    match result {
        Ok(snapshots) => HttpResponse::Ok().json(snapshots),
        Err(e) => {
            error!("Failed to get snapshots: {}", e);
            HttpResponse::InternalServerError().json(e.to_string())
        }
    }
}
