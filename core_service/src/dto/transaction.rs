use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

// DTO for transaction response in API
#[derive(Debug, Deserialize, Serialize, FromRow, ToSchema)]
pub struct TransactionDto {
    pub id: i32,
    pub asset: String,
    pub wallet: String,
    pub amount: f64,
    pub price: f64,
    pub transaction_type: String,
    pub fee: Option<f64>,
    pub notes: Option<String>,
    #[schema(value_type = String)]
    pub created_at: String,
}

// DTO for creating a new transaction via API
#[derive(Debug, Deserialize, Serialize, Validate, ToSchema)]
pub struct CreateTransactionDto {
    #[validate(range(min = 1))]
    pub asset_id: i32,
    #[validate(range(min = 1))]
    pub wallet_id: i32,
    #[validate(range(min = 0.0))]
    pub amount: f64,
    #[validate(range(min = 0.0))]
    pub price: f64,
    #[validate(length(min = 1))]
    pub transaction_type: String,
    #[validate(range(min = 0.0))]
    pub fee: Option<f64>,
    #[validate(length(max = 500))]
    pub notes: Option<String>,
}
