use crate::dto::wallet::{CreateWalletDto, WalletDto};
use crate::repository::wallet::WalletRepository;
use actix_web::web;
use anyhow::Result;
use sqlx::PgPool;

// Service for managing wallets
#[derive(Clone)]
pub struct WalletService {
    pool: web::Data<PgPool>,
}

impl WalletService {
    // Creates a new instance of WalletService
    pub fn new(pool: web::Data<PgPool>) -> Self {
        Self { pool }
    }

    // Creates a new wallet
    pub async fn create(&self, wallet: CreateWalletDto) -> Result<WalletDto> {
        let repo = WalletRepository::new(self.pool.as_ref());
        let record = repo
            .create(wallet.name, wallet.wallet_type, wallet.address)
            .await?;

        // Map the database record to DTO
        Ok(WalletDto {
            id: record.id,
            name: record.name,
            wallet_type: record.wallet_type,
            address: record.address,
            created_at: record.created_at.to_string(),
        })
    }

    // Retrieves all wallets
    pub async fn get_all(&self) -> Result<Vec<WalletDto>> {
        let repo = WalletRepository::new(self.pool.as_ref());
        let wallets = repo.get_all().await?;

        // Map database records to DTOs
        let response = wallets
            .into_iter()
            .map(|record| WalletDto {
                id: record.id,
                name: record.name,
                wallet_type: record.wallet_type,
                address: record.address,
                created_at: record.created_at.to_string(),
            })
            .collect();

        Ok(response)
    }
}
