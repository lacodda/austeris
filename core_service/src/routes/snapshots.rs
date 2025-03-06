use crate::models::snapshot::{PortfolioSnapshot, SnapshotAsset, SnapshotRecord};
use crate::models::transaction::TransactionRecord;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use sqlx::PgPool;
use std::collections::HashMap;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/snapshots").route("", web::post().to(create_snapshot)));
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
        })
    })()
    .await;

    match result {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
