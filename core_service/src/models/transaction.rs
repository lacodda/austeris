use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, sqlx::FromRow)]
pub struct TransactionRecord {
    pub id: i32,
    pub asset: String,
    pub wallet: String,
    pub amount: f64,
    pub price: f64,
    pub transaction_type: String,
    pub fee: Option<f64>,
    pub notes: Option<String>,
    pub created_at: PrimitiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, FromRow, ToSchema)]
pub struct TransactionResponse {
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

#[derive(Debug, Serialize, Deserialize, validator::Validate, ToSchema)]
pub struct CreateTransactionRequest {
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
