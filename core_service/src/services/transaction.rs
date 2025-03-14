use crate::dto::transaction::{CreateTransactionDto, TransactionDto};
use crate::error::AppError;
use crate::repository::asset::AssetRepository;
use crate::repository::transaction::TransactionRepository;
use crate::repository::wallet::WalletRepository;
use actix_web::web;
use anyhow::Result;
use sqlx::PgPool;

// Service for managing transactions
#[derive(Clone)]
pub struct TransactionService {
    pool: web::Data<PgPool>,
}

impl TransactionService {
    // Creates a new instance of TransactionService
    pub fn new(pool: web::Data<PgPool>) -> Self {
        Self { pool }
    }

    // Creates a new transaction with validation
    pub async fn create(
        &self,
        transaction: CreateTransactionDto,
    ) -> Result<TransactionDto, AppError> {
        let asset_repo = AssetRepository::new(self.pool.as_ref());
        let wallet_repo = WalletRepository::new(self.pool.as_ref());
        let transaction_repo = TransactionRepository::new(self.pool.as_ref());

        // Check if asset_id exists
        if !asset_repo
            .exists(transaction.asset_id)
            .await
            .map_err(AppError::internal)?
        {
            return Err(AppError::bad_request(anyhow::anyhow!("Asset not found")));
        }

        // Check if wallet_id exists
        if !wallet_repo
            .exists(transaction.wallet_id)
            .await
            .map_err(AppError::internal)?
        {
            return Err(AppError::bad_request(anyhow::anyhow!("Wallet not found")));
        }

        let record = transaction_repo
            .create(transaction)
            .await
            .map_err(AppError::internal)?;
        Ok(record.into())
    }
}
