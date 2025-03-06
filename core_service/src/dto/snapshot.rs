use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// DTO for a single asset in a snapshot
#[derive(Debug, Deserialize, Serialize, ToSchema, Clone)]
pub struct SnapshotAssetDto {
    pub symbol: String,
    pub amount: f64,
    pub cmc_id: String,
}

// DTO for snapshot response in API
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct SnapshotDto {
    pub id: i32,
    pub created_at: String,
    pub assets: Vec<SnapshotAssetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Vec<SnapshotDiffDto>>, // Difference between snapshot and current portfolio
}

// DTO for difference in asset amounts
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct SnapshotDiffDto {
    pub symbol: String,
    pub amount_diff: f64,
    pub cmc_id: String,
}
