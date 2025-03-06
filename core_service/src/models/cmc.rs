use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcResponse {
    pub status: CmcStatus,
    pub data: Vec<CmcListing>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcStatus {
    pub timestamp: String,
    pub error_code: i32,
    pub error_message: Option<String>,
    pub elapsed: i32,
    pub credit_count: i32,
    pub notice: Option<String>,
    pub total_count: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcListing {
    pub id: i32,
    pub name: String,
    pub symbol: String,
    pub slug: String,
    pub num_market_pairs: i32,
    pub date_added: String,
    pub tags: Vec<String>,
    pub max_supply: Option<f64>,
    pub circulating_supply: f64,
    pub total_supply: f64,
    pub is_active: Option<i32>,
    pub infinite_supply: bool,
    pub platform: Option<CmcPlatform>,
    pub cmc_rank: i32,
    pub is_fiat: Option<i32>,
    pub self_reported_circulating_supply: Option<f64>,
    pub self_reported_market_cap: Option<f64>,
    pub tvl_ratio: Option<f64>,
    pub last_updated: String,
    pub quote: CmcQuoteData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcTag {
    pub slug: String,
    pub name: String,
    pub category: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcPlatform {
    pub id: i32,
    pub name: String,
    pub symbol: String,
    pub slug: String,
    pub token_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcQuoteData {
    #[serde(rename = "USD")]
    pub usd: CmcQuote,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CmcQuote {
    pub price: Option<f64>,
    pub volume_24h: Option<f64>,
    pub volume_change_24h: Option<f64>,
    pub percent_change_1h: Option<f64>,
    pub percent_change_24h: Option<f64>,
    pub percent_change_7d: Option<f64>,
    pub percent_change_30d: Option<f64>,
    pub percent_change_60d: Option<f64>,
    pub percent_change_90d: Option<f64>,
    pub market_cap: Option<f64>,
    pub market_cap_dominance: Option<f64>,
    pub fully_diluted_market_cap: Option<f64>,
    pub tvl: Option<f64>,
    pub last_updated: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcQuoteResponse {
    pub status: CmcStatus,
    pub data: std::collections::HashMap<String, Vec<CmcQuoteListing>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CmcQuoteListing {
    pub id: i32,
    pub name: String,
    pub symbol: String,
    pub slug: String,
    pub num_market_pairs: i32,
    pub date_added: String,
    pub tags: Vec<CmcTag>,
    pub max_supply: Option<f64>,
    pub circulating_supply: f64,
    pub total_supply: f64,
    pub is_active: i32,
    pub infinite_supply: bool,
    pub platform: Option<CmcPlatform>,
    pub cmc_rank: Option<i32>,
    pub is_fiat: i32,
    pub self_reported_circulating_supply: Option<f64>,
    pub self_reported_market_cap: Option<f64>,
    pub tvl_ratio: Option<f64>,
    pub last_updated: String,
    pub quote: CmcQuoteData,
}
