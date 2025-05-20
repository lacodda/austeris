// Tests for configuration loading
use austeris_common::config::Config;
use std::env;

#[tokio::test]
async fn test_config_from_env_success() {
    // Clear all environment variables to ensure a clean state
    let vars: Vec<(String, String)> = env::vars().collect();
    for (key, _) in vars {
        env::remove_var(&key);
    }

    // Set up environment variables
    env::set_var("DATABASE_URL", "postgres://user:password@localhost:5432/austeris");
    env::set_var("REDIS_URL", "redis://localhost:6379");
    env::set_var("APP_PORT", "8080");
    env::set_var("AUSTERIS_DOTENV", "false");

    // Test successful config loading
    let config = Config::from_env().unwrap();
    assert_eq!(config.database_url, "postgres://user:password@localhost:5432/austeris");
    assert_eq!(config.redis_url, "redis://localhost:6379");
    assert_eq!(config.app_port, 8080);
}