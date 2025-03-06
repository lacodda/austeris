use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use std::env;

pub async fn connect() -> Result<sqlx::PgPool> {
    dotenv::dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    Ok(pool)
}
