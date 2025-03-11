use crate::models::wallet::WalletDb;
use anyhow::Result;
use sqlx::PgPool;

// Repository for wallet-related database operations
pub struct WalletRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> WalletRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Checks if a wallet with the given ID exists
    pub async fn exists(&self, wallet_id: i32) -> Result<bool> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM wallets WHERE id = $1")
            .bind(wallet_id)
            .fetch_one(self.pool)
            .await?;
        Ok(count > 0)
    }

    // Creates a new wallet in the database
    pub async fn create(
        &self,
        name: String,
        wallet_type: String,
        address: Option<String>,
    ) -> Result<WalletDb> {
        let record = sqlx::query_as!(
            WalletDb,
            r#"
            INSERT INTO wallets (name, type, address)
            VALUES ($1, $2, $3)
            RETURNING id, name, type AS wallet_type, address, created_at
            "#,
            name,
            wallet_type,
            address
        )
        .fetch_one(self.pool)
        .await?;
        Ok(record)
    }

    // Retrieves all wallets from the database
    pub async fn get_all(&self) -> Result<Vec<WalletDb>> {
        let wallets = sqlx::query_as!(
            WalletDb,
            r#"
            SELECT id, name, type AS wallet_type, address, created_at
            FROM wallets
            "#,
        )
        .fetch_all(self.pool)
        .await?;
        Ok(wallets)
    }
}
