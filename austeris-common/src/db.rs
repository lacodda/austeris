// Database connection utilities for Austeris services
use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use std::env;

pub async fn connect() -> Result<sqlx::PgPool> {
    // Load environment variables from .env file, if present
    dotenvy::dotenv().ok();

    // Retrieve database variables from environment
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let max_connections = env::var("SQLX_MAX_CONNECTIONS")
        .unwrap_or_else(|_| "5".to_string())
        .parse::<u32>()
        .expect("SQLX_MAX_CONNECTIONS must be a valid number");
    let acquire_timeout = env::var("SQLX_ACQUIRE_TIMEOUT")
        .unwrap_or_else(|_| "30".to_string())
        .parse::<u64>()
        .expect("SQLX_ACQUIRE_TIMEOUT must be a valid number");

    // Create a connection pool with specified settings
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(acquire_timeout))
        .connect(&database_url)
        .await?;

    Ok(pool)
}
