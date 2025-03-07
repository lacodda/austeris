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
}
