use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

// DTO for asset response in API
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct AssetDto {
    pub id: i32,
    pub symbol: String,
    pub name: String,
    pub cmc_id: String,
    pub decimals: Option<i32>,
    #[schema(value_type = String)]
    pub created_at: String,
}

// DTO for creating a new asset via API
#[derive(Debug, Deserialize, Serialize, Validate, ToSchema)]
pub struct CreateAssetDto {
    #[validate(length(min = 1, message = "Symbol must not be empty"))]
    pub symbol: String,
    #[validate(length(min = 1, message = "Name must not be empty"))]
    pub name: String,
    #[validate(length(min = 1, message = "CMC ID must not be empty"))]
    pub cmc_id: String,
    #[validate(range(min = 0, message = "Decimals must be non-negative"))]
    pub decimals: Option<i32>,
}
