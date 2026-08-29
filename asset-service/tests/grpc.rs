// Tests for gRPC endpoints
use tonic::Request;
use crate::proto::{asset_service_client::AssetServiceClient, PriceRequest};
use std::net::SocketAddr;

#[tokio::test]
async fn test_get_price() {
    // Requires running gRPC server; skip for simplicity
    let addr = "http://localhost:9081".parse::<SocketAddr>().unwrap();
    let mut client = AssetServiceClient::connect(format!("http://{}", addr)).await.unwrap();
    let request = Request::new(PriceRequest { asset_id: 1 });
    let response = client.get_price(request).await.unwrap();
    assert_eq!(response.get_ref().asset_id, 1);
}