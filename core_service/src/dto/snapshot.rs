use crate::models::snapshot::SnapshotDb;
use crate::utils::datetime::format_iso8601;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// DTO for a single asset in a snapshot
#[derive(Debug, Deserialize, Serialize, ToSchema, Clone)]
pub struct SnapshotAssetDto {
    pub symbol: String,
    pub amount: f64,
    pub cmc_id: i32,
}

// DTO for snapshot response in API
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct SnapshotDto {
    pub id: i32,
    pub created_at: String,
    pub assets: Vec<SnapshotAssetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Vec<SnapshotDiffDto>>,
}

impl From<SnapshotDb> for SnapshotDto {
    fn from(record: SnapshotDb) -> Self {
        let assets = record
            .assets
            .0
            .into_iter()
            .map(|asset| SnapshotAssetDto {
                symbol: asset.symbol,
                amount: asset.amount,
                cmc_id: asset.cmc_id,
            })
            .collect();
        Self {
            id: record.id,
            created_at: format_iso8601(record.created_at),
            assets,
            diff: None,
        }
    }
}

// DTO for difference in asset amounts
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct SnapshotDiffDto {
    pub symbol: String,
    pub amount_diff: f64,
    pub cmc_id: i32,
}
