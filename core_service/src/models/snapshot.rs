use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::types::Json;
use sqlx::FromRow;

// Represents a snapshot record fetched from the database
#[derive(Debug, Deserialize, Serialize, FromRow)]
pub struct SnapshotDb {
    pub id: i32,
    pub created_at: PrimitiveDateTime,
    pub assets: Json<Vec<SnapshotAssetDb>>, // Stored as JSONB in the database
}

// Represents an asset in a snapshot stored in the database
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SnapshotAssetDb {
    pub symbol: String,
    pub amount: f64, // Positive or negative difference
    pub cmc_id: i32,
}
