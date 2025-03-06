use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SnapshotAsset {
    pub symbol: String,
    pub amount: f64,
    pub cmc_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSnapshot {
    pub id: i32,
    pub created_at: String,
    pub assets: Vec<SnapshotAsset>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SnapshotRecord {
    pub id: i32,
    pub created_at: sqlx::types::time::PrimitiveDateTime,
    pub assets: Json<Vec<SnapshotAsset>>,
}
