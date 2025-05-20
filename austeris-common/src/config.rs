// Environment configuration for Austeris services
use dotenvy::dotenv;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub app_port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        // Only load .env if explicitly enabled via environment variable
        if env::var("AUSTERIS_DOTENV").map(|v| v == "true").unwrap_or(false) {
            dotenv().ok();
        }

        let database_url = env::var("DATABASE_URL")?;
        let redis_url = env::var("REDIS_URL")?;
        let app_port = env::var("APP_PORT")?.parse::<u16>().map_err(|_| {
            env::VarError::NotPresent
        })?;

        Ok(Config {
            database_url,
            redis_url,
            app_port,
        })
    }
}
