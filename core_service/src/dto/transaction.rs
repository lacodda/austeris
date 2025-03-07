use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::{Validate, ValidationError};

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
    #[validate(range(min = 1, message = "Asset ID must be positive"))]
    pub asset_id: i32,
    #[validate(range(min = 1, message = "Wallet ID must be positive"))]
    pub wallet_id: i32,
    #[validate(range(min = 0.0, message = "Amount must be non-negative"))]
    pub amount: f64,
    #[validate(range(min = 0.0, message = "Price must be non-negative"))]
    pub price: f64,
    #[validate(custom(
        function = "validate_transaction_type",
        message = "Transaction type must be either 'BUY' or 'SELL'"
    ))]
    pub transaction_type: String,
    #[validate(range(min = 0.0, message = "Fee must be non-negative"))]
    pub fee: Option<f64>,
    #[validate(length(max = 500))]
    pub notes: Option<String>,
}

// Custom validation function for transaction_type
fn validate_transaction_type(transaction_type: &str) -> Result<(), ValidationError> {
    if transaction_type == "BUY" || transaction_type == "SELL" {
        Ok(())
    } else {
        Err(ValidationError::new("transaction_type"))
    }
}
