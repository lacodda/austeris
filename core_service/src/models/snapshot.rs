use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Vec<SnapshotDiff>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SnapshotDiff {
    pub symbol: String,
    pub amount_diff: f64,
    pub cmc_id: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SnapshotRecord {
    pub id: i32,
    pub created_at: sqlx::types::time::PrimitiveDateTime,
    pub assets: Json<Vec<SnapshotAsset>>,
}
