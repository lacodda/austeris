use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;

// Represents a wallet record fetched from the database
#[derive(Debug, Deserialize, Serialize, FromRow)]
pub struct WalletDb {
    pub id: i32,
    pub name: String,
    pub wallet_type: String,
    pub address: Option<String>,
    pub created_at: PrimitiveDateTime,
}
