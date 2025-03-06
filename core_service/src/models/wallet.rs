use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use utoipa::ToSchema;

// Internal structure for mapping database rows to wallet data
#[derive(Debug, sqlx::FromRow)]
pub struct WalletRecord {
    pub id: i32,
    pub name: String,
    pub wallet_type: String,
    pub address: Option<String>,
    pub created_at: PrimitiveDateTime,
}

// Response structure for wallet data returned to the client
#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct WalletResponse {
    pub id: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub wallet_type: String,
    pub address: Option<String>,
    #[schema(value_type = String)]
    pub created_at: String,
}

// Request structure for creating a new wallet
#[derive(Debug, Serialize, Deserialize, validator::Validate, ToSchema)]
pub struct CreateWalletRequest {
    #[validate(length(min = 1, max = 50))]
    pub name: String,
    #[validate(length(min = 1, max = 20))]
    pub wallet_type: String,
    #[validate(length(max = 255))]
    pub address: Option<String>,
}
