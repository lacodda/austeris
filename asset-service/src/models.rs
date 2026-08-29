use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use validator::Validate;

// Represents an asset record fetched from the database
#[derive(Debug, Deserialize, Serialize, FromRow)]
pub struct AssetDb {
    pub id: i32,
    pub symbol: String,
    pub name: String,
    pub cmc_id: i32,
    pub decimals: Option<i32>,
    pub rank: Option<i32>,
    pub created_at: PrimitiveDateTime,
}

// Query parameters for GET /assets/prices
#[derive(Debug, Deserialize, Validate)]
pub struct PriceQueryParams {
    pub asset_ids: Option<String>,
}

// Query parameters for GET /assets/prices/history
#[derive(Debug, Deserialize, Validate)]
pub struct HistoryQueryParams {
    pub asset_ids: Option<String>,
    pub start_date: String,
    pub end_date: Option<String>,
}
