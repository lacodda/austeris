use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

// DTO for asset response in API
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct AssetDto {
    pub id: i32,
    pub symbol: String,
    pub name: String,
    pub cmc_id: i32,
    pub decimals: Option<i32>,
    pub rank: Option<i32>,
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
    #[validate(range(min = 1, message = "CMC ID must be a positive integer"))]
    pub cmc_id: i32,
    #[validate(range(min = 0, message = "Decimals must be non-negative"))]
    pub decimals: Option<i32>,
    #[validate(range(min = 0, message = "Rank must be non-negative"))]
    pub rank: Option<i32>,
}

// DTO for the response of updating assets
#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateAssetsResponse {
    pub updated_count: usize,
    #[schema(value_type = String)]
    pub updated_at: String,
}

// DTO for asset price response with asset details in API
#[derive(Debug, Serialize, ToSchema)]
pub struct AssetPriceWithDetailsDto {
    pub cmc_id: i32,
    pub symbol: String,
    pub name: String,
    pub price_usd: f64,
    #[schema(value_type = String)]
    pub timestamp: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AssetPriceHistoryDto {
    pub cmc_id: i32,
    pub symbol: String,
    pub price_usd: f64,
    #[schema(value_type = String)]
    pub timestamp: String,
}
