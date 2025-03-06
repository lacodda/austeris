use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use std::env;

// Connect to the PostgreSQL database using the DATABASE_URL environment variable
pub async fn connect() -> Result<sqlx::PgPool> {
    // Load environment variables from .env file, if present
    dotenv::dotenv().ok();

    // Retrieve database URL from environment
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    // Create a connection pool with a maximum of 5 connections
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    Ok(pool)
}
