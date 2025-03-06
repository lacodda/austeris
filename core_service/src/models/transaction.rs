use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use utoipa::ToSchema;

// Represents a transaction record fetched from the database
#[derive(Debug, FromRow, Deserialize, Serialize)]
pub struct TransactionDb {
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

// Represents query parameters for filtering transactions
#[derive(Debug, Deserialize, ToSchema)]
pub struct FilterParams {
    pub asset_id: Option<i32>,
    pub wallet_id: Option<i32>,
    pub start_date: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
