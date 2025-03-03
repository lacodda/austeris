use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CmcListing {
    pub id: i32,
    pub name: String,
    pub symbol: String,
}

#[derive(Debug, Deserialize)]
pub struct CmcResponse {
    pub data: Vec<CmcListing>,
}
