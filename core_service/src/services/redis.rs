use anyhow::Result;
use redis::{AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Serialize, Deserialize, Debug)]
pub struct CachedPrice {
    pub price_usd: f64,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct RedisService {
    client: Client,
}

impl RedisService {
    // Creates a new RedisService instance using REDIS_URL from environment
    pub fn new() -> Result<Self> {
        let redis_url =
            env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let client = Client::open(redis_url)?;
        Ok(Self { client })
    }

    // Saves a price to Redis with a TTL of 1 hour
    pub async fn save_price(&self, asset_id: i32, price_usd: f64, timestamp: String) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("asset_price:{}", asset_id);
        let cached_price = CachedPrice {
            price_usd,
            timestamp,
        };
        let serialized = serde_json::to_string(&cached_price)?;
        // Explicitly specify the return type as () to avoid never type fallback
        let _: () = redis::cmd("SETEX")
            .arg(&key)
            .arg(3600)
            .arg(&serialized)
            .query_async(&mut conn)
            .await
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    // Retrieves a price from Redis by asset_id
    pub async fn get_price(&self, asset_id: i32) -> Result<Option<CachedPrice>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("asset_price:{}", asset_id);
        let result: Option<String> = conn.get(key).await?;
        match result {
            Some(data) => {
                let cached_price: CachedPrice = serde_json::from_str(&data)?;
                Ok(Some(cached_price))
            }
            None => Ok(None),
        }
    }
}
