use crate::dto::transaction::{CreateTransactionDto, TransactionDto};
use crate::error::AppError;
use crate::repository::asset::AssetRepository;
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

        // Insert the transaction into the database and fetch full details
        let record = sqlx::query!(
            r#"
            INSERT INTO transactions 
                (asset_id, wallet_id, amount, price, type, fee, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING 
                id, 
                (SELECT symbol FROM assets WHERE id = $1) AS asset,
                (SELECT name FROM wallets WHERE id = $2) AS wallet,
                amount,
                price,
                type AS transaction_type,
                fee,
                notes,
                created_at
            "#,
            transaction.asset_id,
            transaction.wallet_id,
            transaction.amount,
            transaction.price,
            transaction.transaction_type,
            transaction.fee,
            transaction.notes,
        )
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(AppError::internal)?;

        // Map the database record to DTO
        Ok(TransactionDto {
            id: record.id,
            asset: record.asset.unwrap(),
            wallet: record.wallet.unwrap(),
            amount: record.amount,
            price: record.price,
            transaction_type: record.transaction_type,
            fee: record.fee,
            notes: record.notes,
            created_at: record.created_at.to_string(),
        })
    }
}
