use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::{Validate, ValidationError};

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
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct FilterParams {
    #[validate(range(min = 1, message = "Asset ID must be positive"))]
    pub asset_id: Option<i32>,
    #[validate(range(min = 1, message = "Wallet ID must be positive"))]
    pub wallet_id: Option<i32>,
    #[validate(custom(
        function = "validate_date",
        message = "Invalid start_date format, expected ISO 8601 (e.g., '2024-01-01T00:00:00' or '2024-01-01T00:00:00Z')"
    ))]
    pub start_date: Option<String>,
    #[validate(range(min = 1, message = "Limit must be positive"))]
    pub limit: Option<i64>,
    #[validate(range(min = 0, message = "Offset must be non-negative"))]
    pub offset: Option<i64>,
}

// Custom validation function for ISO 8601 date
fn validate_date(date: &str) -> Result<(), ValidationError> {
    if DateTime::parse_from_rfc3339(date).is_ok() {
        return Ok(());
    }
    NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S")
        .map(|_| ())
        .map_err(|_| ValidationError::new("start_date"))
}
