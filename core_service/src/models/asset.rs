use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;

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
