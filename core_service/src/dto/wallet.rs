use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

// DTO for wallet response in API
#[derive(Debug, Deserialize, Serialize, FromRow, ToSchema)]
pub struct WalletDto {
    pub id: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub wallet_type: String,
    pub address: Option<String>,
    #[schema(value_type = String)]
    pub created_at: String,
}

// DTO for creating a new wallet via API
#[derive(Debug, Deserialize, Serialize, Validate, ToSchema)]
pub struct CreateWalletDto {
    #[validate(length(min = 1, max = 50))]
    pub name: String,
    #[validate(length(min = 1, max = 20))]
    pub wallet_type: String,
    #[validate(length(max = 255))]
    pub address: Option<String>,
}
