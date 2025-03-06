use serde::{Deserialize, Serialize};
use sqlx::types::Json;

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotAsset {
    pub symbol: String,
    pub amount: f64,
    pub cmc_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub id: i32,
    pub created_at: String,
    pub assets: Json<Vec<SnapshotAsset>>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SnapshotRecord {
    pub id: i32,
    pub created_at: sqlx::types::time::PrimitiveDateTime,
    pub assets: Json<Vec<SnapshotAsset>>,
}
