use crate::models::snapshot::{PortfolioSnapshot, SnapshotAsset, SnapshotDiff, SnapshotRecord};
use crate::models::transaction::TransactionRecord;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashMap;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/snapshots")
            .route("", web::post().to(create_snapshot))
            .route("", web::get().to(get_snapshots)),
    );
}

#[utoipa::path(
    post,
    path = "/snapshots",
    responses(
        (status = 200, description = "Snapshot created successfully", body = PortfolioSnapshot, example = json!({"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": "1"}, {"symbol": "ETH", "amount": 10.0, "cmc_id": "1027"}]})),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to save snapshot to database"))
    )
)]
async fn create_snapshot(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<PortfolioSnapshot> = (|| async {
        let transactions = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT 
                t.id, 
                a.symbol as asset, 
                w.name as wallet,
                t.amount,
                t.price,
                t.type as transaction_type,
                t.fee,
                t.notes,
                t.created_at
            FROM transactions t
            JOIN assets a ON t.asset_id = a.id
            JOIN wallets w ON t.wallet_id = w.id
            "#,
        )
        .fetch_all(pool.get_ref())
        .await?;

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

        let snapshot_assets: Vec<SnapshotAsset> = asset_amounts
            .into_iter()
            .filter(|(_, (amount, _))| *amount > 0.0)
            .map(|(symbol, (amount, cmc_id))| SnapshotAsset {
                symbol,
                amount,
                cmc_id,
            })
            .collect();

        let record = sqlx::query_as::<_, SnapshotRecord>(
            r#"
            INSERT INTO portfolio_snapshots (assets)
            VALUES ($1)
            RETURNING id, created_at, assets
            "#,
        )
        .bind(sqlx::types::Json(&snapshot_assets))
        .fetch_one(pool.get_ref())
        .await?;

        Ok(PortfolioSnapshot {
            id: record.id,
            created_at: record.created_at.to_string(),
            assets: snapshot_assets,
            diff: None,
        })
    })()
    .await;

    match result {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    get,
    path = "/snapshots",
    responses(
        (status = 200, description = "Successfully retrieved list of snapshots with differences", body = Vec<PortfolioSnapshot>, example = json!([{"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": "1"}, {"symbol": "ETH", "amount": 10.0, "cmc_id": "1027"}], "diff": [{"symbol": "BTC", "amount_diff": -0.5, "cmc_id": "1"}, {"symbol": "ETH", "amount_diff": 2.0, "cmc_id": "1027"}]}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to fetch snapshots from database"))
    )
)]
async fn get_snapshots(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<Vec<PortfolioSnapshot>> = (|| async {
        let snapshots = sqlx::query_as::<_, SnapshotRecord>(
            r#"
            SELECT id, created_at, assets
            FROM portfolio_snapshots
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(pool.get_ref())
        .await?;

        let transactions = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT 
                t.id, 
                a.symbol as asset, 
                w.name as wallet,
                t.amount,
                t.price,
                t.type as transaction_type,
                t.fee,
                t.notes,
                t.created_at
            FROM transactions t
            JOIN assets a ON t.asset_id = a.id
            JOIN wallets w ON t.wallet_id = w.id
            "#,
        )
        .fetch_all(pool.get_ref())
        .await?;

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

        let snapshots_with_diff: Vec<PortfolioSnapshot> = snapshots
            .into_iter()
            .map(|record| {
                let snapshot_assets = record.assets.0.clone();
                let mut diff_map: HashMap<String, SnapshotDiff> = HashMap::new();

                for asset in &snapshot_assets {
                    let current = current_assets
                        .get(&asset.symbol)
                        .map(|(amt, _)| *amt)
                        .unwrap_or(0.0);
                    let diff = current - asset.amount;
                    if diff != 0.0 {
                        diff_map.insert(
                            asset.symbol.clone(),
                            SnapshotDiff {
                                symbol: asset.symbol.clone(),
                                amount_diff: diff,
                                cmc_id: asset.cmc_id.clone(),
                            },
                        );
                    }
                }

                for (symbol, (current_amount, cmc_id)) in &current_assets {
                    if *current_amount > 0.0 && !snapshot_assets.iter().any(|a| &a.symbol == symbol)
                    {
                        diff_map.insert(
                            symbol.clone(),
                            SnapshotDiff {
                                symbol: symbol.clone(),
                                amount_diff: *current_amount,
                                cmc_id: cmc_id.clone(),
                            },
                        );
                    }
                }

                PortfolioSnapshot {
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
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
