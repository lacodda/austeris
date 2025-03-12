use crate::dto::asset::{
    AssetDto, AssetPriceHistoryDto, AssetPriceWithDetailsDto, CreateAssetDto, UpdateAssetsResponse,
};
use crate::error::AppError;
use crate::models::asset::{HistoryQueryParams, PriceQueryParams};
use crate::services::asset::AssetService;
use actix_web::{web, HttpResponse, Responder};
use actix_web_validator::{Json, Query};
use anyhow::Result;

// Configures routes for the /assets scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assets")
            .route("", web::get().to(get_assets))
            .route("", web::post().to(create_asset))
            .route("/update", web::post().to(update_assets))
            .route("/prices", web::get().to(get_asset_prices))
            .route("/prices/history", web::get().to(get_price_history)),
    );
}

// Handles GET /assets to retrieve all assets
#[utoipa::path(
    get,
    path = "/assets",
    responses(
        (status = 200, description = "Successfully retrieved list of assets", body = Vec<AssetDto>, example = json!([{"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": 1, "decimals": 8, "rank": 1, "created_at": "2024-01-01T00:00:00"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Database connection failed"}))
    )
)]
async fn get_assets(asset_service: web::Data<AssetService>) -> Result<impl Responder, AppError> {
    let assets = asset_service.get_all().await.map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(assets))
}

// Handles POST /assets to create a new asset
#[utoipa::path(
    post,
    path = "/assets",
    request_body(
        content = CreateAssetDto,
        description = "Details of the asset to create",
        example = json!({"symbol": "BTC", "name": "Bitcoin", "cmc_id": 1, "decimals": 8})
    ),
    responses(
        (status = 200, description = "Asset created successfully", body = AssetDto, example = json!({"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": 1, "decimals": 8, "rank": 1, "created_at": "2024-01-01T00:00:00"})),
        (status = 400, description = "Invalid request data (e.g., missing required fields)", body = String, example = json!({"status": 400, "error": "Bad Request", "message": "Validation error: Symbol must not be empty"})),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to insert asset into database"}))
    )
)]
async fn create_asset(
    asset_service: web::Data<AssetService>,
    asset: Json<CreateAssetDto>,
) -> Result<impl Responder, AppError> {
    let response = asset_service
        .create(asset.into_inner())
        .await
        .map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(response))
}

// Handles POST /assets/update to sync assets with CoinMarketCap
#[utoipa::path(
    post,
    path = "/assets/update",
    responses(
        (status = 200, description = "Assets updated successfully from CoinMarketCap", body = UpdateAssetsResponse, example = json!({"updated_count": 1000, "updated_at": "2025-03-07T12:00:00Z"})),
        (status = 500, description = "Internal server error (e.g., CoinMarketCap API or database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to fetch listings from CoinMarketCap"}))
    )
)]
async fn update_assets(asset_service: web::Data<AssetService>) -> Result<impl Responder, AppError> {
    let response = asset_service.update().await.map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(response))
}

// Handles GET /assets/prices to retrieve latest asset prices with details
#[utoipa::path(
    get,
    path = "/assets/prices",
    params(
        ("asset_ids", Query, description = "Comma-separated list of asset IDs to filter by (e.g., 1,2)", example = "1,2")
    ),
    responses(
        (status = 200, description = "Successfully retrieved latest asset prices with details", body = Vec<AssetPriceWithDetailsDto>, example = json!([{"cmc_id": 1, "symbol": "BTC", "name": "Bitcoin", "price_usd": 60000.0, "timestamp": "2025-03-08T12:00:00Z"}, {"cmc_id": 1027, "symbol": "ETH", "name": "Ethereum", "price_usd": 3000.0, "timestamp": "2025-03-08T12:00:00Z"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Database connection failed"}))
    )
)]
async fn get_asset_prices(
    asset_service: web::Data<AssetService>,
    query: Query<PriceQueryParams>,
) -> Result<impl Responder, AppError> {
    let response = asset_service
        .get_prices(query.into_inner())
        .await
        .map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(response))
}

// Handles GET /assets/prices/history to retrieve history asset prices with details
#[utoipa::path(
    get,
    path = "/assets/prices/history",
    params(
        ("asset_ids", Query, description = "Comma-separated list of asset IDs to filter by (e.g., 1,2)", example = "1,2"),
        ("start_date", Query, description = "Start date in ISO 8601 format (e.g., 2025-03-01T00:00:00Z)", example = "2025-03-01T00:00:00Z"),
        ("end_date", Query, description = "End date in ISO 8601 format (optional, defaults to now)", example = "2025-03-08T00:00:00Z")
    ),
    responses(
        (status = 200, description = "Successfully retrieved historical asset prices", body = Vec<AssetPriceHistoryDto>, example = json!([{"cmc_id": 1, "symbol": "BTC", "price_usd": 59000.0, "timestamp": "2025-03-01T00:00:00Z"}, {"cmc_id": 1, "symbol": "BTC", "price_usd": 60000.0, "timestamp": "2025-03-08T00:00:00Z"}])),
        (status = 400, description = "Invalid date format", body = String, example = json!({"status": 400, "error": "Bad Request", "message": "Invalid start_date format"})),
        (status = 500, description = "Internal server error", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Database connection failed"}))
    )
)]
async fn get_price_history(
    asset_service: web::Data<AssetService>,
    query: Query<HistoryQueryParams>,
) -> Result<impl Responder, AppError> {
    let response = asset_service
        .get_price_history(query.into_inner())
        .await
        .map_err(AppError::internal)?;

    Ok(HttpResponse::Ok().json(response))
}
