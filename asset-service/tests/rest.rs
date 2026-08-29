// Tests for REST API endpoints
use actix_web::{test, App};
use actix_web_validator::Json;
use austeris_common::{db, error::AppError};
use redis::Client;
use sqlx::PgPool;
use crate::{dto::CreateAssetDto, routes, services::AssetService};

#[tokio::test]
async fn test_get_assets() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    let pool = db::connect().await.unwrap();
    let redis = Client::open("redis://localhost:6379").unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(redis))
            .configure(routes::config)
    ).await;

    let req = test::TestRequest::get().uri("/assets").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_create_asset() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return;
    }

    let pool = db::connect().await.unwrap();
    let redis = Client::open("redis://localhost:6379").unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(redis))
            .configure(routes::config)
    ).await;

    let asset = CreateAssetDto {
        symbol: "BTC".to_string(),
        name: "Bitcoin".to_string(),
        cmc_id: 1,
        decimals: None,
        rank: Some(1),
    };
    let req = test::TestRequest::post()
        .uri("/assets")
        .set_json(&asset)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);
}
