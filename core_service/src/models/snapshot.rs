use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use utoipa::ToSchema;

// Represents an asset in a portfolio snapshot
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct SnapshotAsset {
    pub symbol: String,
    pub amount: f64,
    pub cmc_id: String,
}

// Represents a portfolio snapshot with optional difference from current state
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PortfolioSnapshot {
    pub id: i32,
    pub created_at: String,
    pub assets: Vec<SnapshotAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Vec<SnapshotDiff>>, // Difference between snapshot and current portfolio
}

// Represents the difference in asset amounts between a snapshot and current portfolio
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SnapshotDiff {
    pub symbol: String,
    pub amount_diff: f64, // Positive or negative difference
    pub cmc_id: String,
}

// Internal structure for mapping database rows to snapshot data
#[derive(Debug, sqlx::FromRow)]
pub struct SnapshotRecord {
    pub id: i32,
    pub created_at: sqlx::types::time::PrimitiveDateTime,
    pub assets: Json<Vec<SnapshotAsset>>, // Stored as JSONB in the database
}
