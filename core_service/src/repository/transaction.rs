use crate::models::transaction::{FilterParams, TransactionRecord};
use anyhow::Result;
use sqlx::types::time::PrimitiveDateTime;
use sqlx::PgPool;
use time::format_description::well_known::Iso8601;

// Repository for transaction-related database operations
pub struct TransactionRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> TransactionRepository<'a> {
    // Creates a new instance of TransactionRepository
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Fetches transactions with optional filters
    pub async fn get_transactions(&self, filters: FilterParams) -> Result<Vec<TransactionRecord>> {
        let mut sql = String::from(
            r#"
            SELECT 
                t.id, 
                a.symbol as asset, 
                w.name as wallet,
                t.amount,
                t.price,
                t.type as transaction_type,
                t.fee,
                t.notes,
                t.created_at
            FROM transactions t
            JOIN assets a ON t.asset_id = a.id
            JOIN wallets w ON t.wallet_id = w.id
            WHERE 1=1
            "#,
        );

        if let Some(asset_id) = filters.asset_id {
            sql.push_str(&format!(" AND t.asset_id = {}", asset_id));
        }
        if let Some(wallet_id) = filters.wallet_id {
            sql.push_str(&format!(" AND t.wallet_id = {}", wallet_id));
        }
        if let Some(start_date) = filters.start_date {
            let parsed_date = PrimitiveDateTime::parse(&start_date, &Iso8601::DEFAULT)
                .map_err(|_| anyhow::anyhow!("Invalid start_date format"))?;
            sql.push_str(&format!(" AND t.created_at >= '{}'", parsed_date));
        }
        sql.push_str(&format!(
            " LIMIT {} OFFSET {}",
            filters.limit.unwrap_or(10),
            filters.offset.unwrap_or(0)
        ));

        let transactions = sqlx::query_as::<_, TransactionRecord>(&sql)
            .fetch_all(self.pool)
            .await?;
        Ok(transactions)
    }

    // Fetches all transactions without filters for portfolio calculations
    pub async fn get_all_transactions(&self) -> Result<Vec<TransactionRecord>> {
        let transactions = sqlx::query_as::<_, TransactionRecord>(
            r#"
            SELECT 
                t.id, 
                a.symbol as asset, 
                w.name as wallet,
                t.amount,
                t.price,
                t.type as transaction_type,
                t.fee,
                t.notes,
                t.created_at
            FROM transactions t
            JOIN assets a ON t.asset_id = a.id
            JOIN wallets w ON t.wallet_id = w.id
            "#,
        )
        .fetch_all(self.pool)
        .await?;
        Ok(transactions)
    }
}
