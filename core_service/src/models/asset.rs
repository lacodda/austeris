use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use utoipa::ToSchema;

// Internal structure for mapping database rows to asset data
#[derive(Debug, sqlx::FromRow)]
pub struct AssetRecord {
    pub id: i32,
    pub symbol: String,
    pub name: String,
    pub cmc_id: String,
    pub decimals: Option<i32>,
    pub created_at: PrimitiveDateTime,
}

// Response structure for asset data returned to the client
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AssetResponse {
    pub id: i32,
    pub symbol: String,
    pub name: String,
    pub cmc_id: String,
    pub decimals: Option<i32>,
    #[schema(value_type = String)]
    pub created_at: String,
}

// Response structure for asset data returned to the client
#[derive(Debug, Serialize, Deserialize, validator::Validate, ToSchema)]
pub struct CreateAssetRequest {
    #[validate(length(min = 1, max = 10))]
    pub symbol: String,
    #[validate(length(min = 1, max = 50))]
    pub name: String,
    #[validate(length(min = 1, max = 50))]
    pub cmc_id: String,
    #[validate(range(min = 0, max = 18))]
    pub decimals: Option<i32>,
}
