use crate::dto::snapshot::{SnapshotAssetDto, SnapshotDiffDto, SnapshotDto};
use crate::error::AppError;
use crate::models::snapshot::SnapshotDb;
use crate::services::portfolio::PortfolioService;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
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
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to save snapshot to database"}))
    )
)]
async fn create_snapshot(
    pool: web::Data<PgPool>,
    portfolio: web::Data<PortfolioService>,
) -> Result<impl Responder, AppError> {
    // Get current asset snapshot from PortfolioService
    let snapshot_assets = portfolio
        .get_current_snapshot()
        .await
        .map_err(AppError::internal)?;

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
    .await
    .map_err(AppError::internal)?;

    let response = SnapshotDto {
        id: record.id,
        created_at: record.created_at.to_string(),
        assets: snapshot_assets,
        diff: None,
    };

    Ok(HttpResponse::Ok().json(response))
}

// Handles GET /snapshots to retrieve all snapshots with differences
#[utoipa::path(
    get,
    path = "/snapshots",
    responses(
        (status = 200, description = "Successfully retrieved list of snapshots with differences", body = Vec<SnapshotDto>, example = json!([{"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": "1"}, {"symbol": "ETH", "amount": 10.0, "cmc_id": "1027"}], "diff": [{"symbol": "BTC", "amount_diff": -0.5, "cmc_id": "1"}, {"symbol": "ETH", "amount_diff": 2.0, "cmc_id": "1027"}]}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to fetch snapshots from database"}))
    )
)]
async fn get_snapshots(
    pool: web::Data<PgPool>,
    portfolio: web::Data<PortfolioService>,
) -> Result<impl Responder, AppError> {
    // Fetch all snapshots from the database
    let snapshots = sqlx::query_as::<_, SnapshotDb>(
        r#"
        SELECT id, created_at, assets
        FROM portfolio_snapshots
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::internal)?;

    // Get current asset holdings from PortfolioService
    let current_assets = portfolio
        .get_current_assets()
        .await
        .map_err(AppError::internal)?;

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
                if *current_amount > 0.0 && !snapshot_assets.iter().any(|a| &a.symbol == symbol) {
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

    Ok(HttpResponse::Ok().json(snapshots_with_diff))
}
