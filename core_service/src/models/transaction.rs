use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, FromRow)]
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

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct FilterParams {
    pub asset_id: Option<i32>,
    pub wallet_id: Option<i32>,
    pub start_date: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
