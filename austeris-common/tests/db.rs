// Tests for database connection
use austeris_common::db;
use std::env;

#[tokio::test]
async fn test_db_connection() {
    // Skip test if no test database is available
    if env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    env::set_var("DATABASE_URL", env::var("TEST_DATABASE_URL").unwrap());
    env::set_var("SQLX_MAX_CONNECTIONS", "2");
    env::set_var("SQLX_ACQUIRE_TIMEOUT", "10");

    let pool = db::connect().await.unwrap();
    let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(row.0, 1);

    pool.close().await;
}
