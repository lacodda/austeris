use crate::dto::transaction::CreateTransactionDto;
use crate::models::transaction::{FilterParams, TransactionDb};
use crate::utils::datetime::parse_iso8601;
use anyhow::Result;
use sqlx::{PgPool, Postgres, QueryBuilder};

// Repository for transaction-related database operations
pub struct TransactionRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> TransactionRepository<'a> {
    // Creates a new instance of TransactionRepository
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    // Creates a new transaction in the database
    pub async fn create(&self, transaction: CreateTransactionDto) -> Result<TransactionDb> {
        let record = sqlx::query_as!(
            TransactionDb,
            r#"
            INSERT INTO transactions 
                (asset_id, wallet_id, amount, price, type, fee, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING 
                id, 
                (SELECT symbol FROM assets WHERE id = $1) AS "asset!",
                (SELECT name FROM wallets WHERE id = $2) AS "wallet!",
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
        .fetch_one(self.pool)
        .await?;
        Ok(record)
    }

    // Fetches transactions with optional filters
    pub async fn get_transactions(&self, filters: FilterParams) -> Result<Vec<TransactionDb>> {
        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT 
                t.id, 
                a.symbol AS asset, 
                w.name AS wallet,
                t.amount,
                t.price,
                t.type AS transaction_type,
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
            query_builder.push(" AND t.asset_id = ");
            query_builder.push_bind(asset_id);
        }

        if let Some(wallet_id) = filters.wallet_id {
            query_builder.push(" AND t.wallet_id = ");
            query_builder.push_bind(wallet_id);
        }

        if let Some(start_date) = filters.start_date {
            let parsed_date = parse_iso8601(&start_date)?;
            query_builder.push(" AND t.created_at >= ");
            query_builder.push_bind(parsed_date);
        }

        query_builder.push(" LIMIT ");
        query_builder.push_bind(filters.limit.unwrap_or(10));

        query_builder.push(" OFFSET ");
        query_builder.push_bind(filters.offset.unwrap_or(0));

        let query = query_builder.build_query_as::<TransactionDb>();
        let transactions = query.fetch_all(self.pool).await?;
        Ok(transactions)
    }

    // Fetches all transactions without filters for portfolio calculations
    pub async fn get_all_transactions(&self) -> Result<Vec<TransactionDb>> {
        let transactions = sqlx::query_as::<_, TransactionDb>(
            r#"
            SELECT 
                t.id, 
                a.symbol AS asset, 
                w.name AS wallet,
                t.amount,
                t.price,
                t.type AS transaction_type,
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
