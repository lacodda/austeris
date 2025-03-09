use crate::dto::asset::{
    AssetDto, AssetPriceHistoryDto, AssetPriceWithDetailsDto, CreateAssetDto, UpdateAssetsResponse,
};
use crate::error::AppError;
use crate::models::asset::AssetDb;
use crate::repository::asset_price::AssetPriceRepository;
use crate::services::cmc::update_assets;
use actix_web::{web, HttpResponse, Responder};
use actix_web_validator::Json;
use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use sqlx::types::time::PrimitiveDateTime;
use sqlx::PgPool;
use time::format_description::well_known::Iso8601;

// Query parameters for GET /assets/prices
#[derive(Debug, Deserialize)]
struct PriceQuery {
    asset_ids: Option<String>,
}

// Query parameters for GET /assets/prices/history
#[derive(Debug, Deserialize)]
struct HistoryQuery {
    asset_ids: Option<String>,
    start_date: String,
    end_date: Option<String>,
}

// Configures routes for the /assets scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assets")
            .route("", web::get().to(get_assets))
            .route("", web::post().to(create_asset))
            .route("/update", web::post().to(update_assets_handler))
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
async fn get_assets(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    // Fetch all assets from the database
    let assets = sqlx::query_as!(
        AssetDb,
        r#"
        SELECT id, symbol, name, cmc_id, decimals, rank, created_at
        FROM assets
        ORDER BY id ASC
        "#
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::internal)?;

    // Map database records to API response format (DTO)
    let response = assets
        .into_iter()
        .map(|record| AssetDto {
            id: record.id,
            symbol: record.symbol,
            name: record.name,
            cmc_id: record.cmc_id,
            decimals: record.decimals,
            rank: record.rank,
            created_at: record.created_at.to_string(),
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(response))
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
    pool: web::Data<PgPool>,
    asset: Json<CreateAssetDto>,
) -> Result<impl Responder, AppError> {
    // Insert the new asset into the database and return its details
    let record = sqlx::query_as!(
        AssetDb,
        r#"
        INSERT INTO assets (symbol, name, cmc_id, decimals, rank)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, symbol, name, cmc_id, decimals, rank, created_at
        "#,
        asset.symbol,
        asset.name,
        asset.cmc_id,
        asset.decimals,
        asset.rank
    )
    .fetch_one(pool.get_ref())
    .await
    .map_err(AppError::internal)?;

    let response = AssetDto {
        id: record.id,
        symbol: record.symbol,
        name: record.name,
        cmc_id: record.cmc_id,
        decimals: record.decimals,
        rank: record.rank,
        created_at: record.created_at.to_string(),
    };

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
async fn update_assets_handler(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    let updated_count = update_assets(pool.get_ref()).await?;
    let updated_at = Utc::now();

    let response = UpdateAssetsResponse {
        updated_count,
        updated_at: updated_at.to_rfc3339(),
    };

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
    pool: web::Data<PgPool>,
    query: web::Query<PriceQuery>,
) -> Result<impl Responder, AppError> {
    let price_repo = AssetPriceRepository::new(pool.get_ref());

    // Parse asset_ids from query parameter if provided
    let asset_ids = query.asset_ids.as_ref().map(|ids| {
        ids.split(',')
            .filter_map(|id| id.trim().parse::<i32>().ok())
            .collect::<Vec<i32>>()
    });

    let prices = price_repo
        .get_latest_prices_with_assets(asset_ids)
        .await
        .map_err(AppError::internal)?;

    let response = prices
        .into_iter()
        .map(
            |(cmc_id, symbol, name, price_usd, timestamp)| AssetPriceWithDetailsDto {
                cmc_id,
                symbol,
                name,
                price_usd,
                timestamp: timestamp.to_string(),
            },
        )
        .collect::<Vec<_>>();

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
    pool: web::Data<PgPool>,
    query: web::Query<HistoryQuery>,
) -> Result<impl Responder, AppError> {
    let price_repo = AssetPriceRepository::new(pool.get_ref());

    let asset_ids = query.asset_ids.as_ref().map(|ids| {
        ids.split(',')
            .filter_map(|id| id.trim().parse::<i32>().ok())
            .collect::<Vec<i32>>()
    });

    let start_date = PrimitiveDateTime::parse(&query.start_date, &Iso8601::DEFAULT)
        .map_err(|e| AppError::bad_request(anyhow::anyhow!("Invalid start_date format: {}", e)))?;

    let end_date = query
        .end_date
        .as_ref()
        .map(|end| {
            PrimitiveDateTime::parse(end, &Iso8601::DEFAULT).map_err(|e| {
                AppError::bad_request(anyhow::anyhow!("Invalid end_date format: {}", e))
            })
        })
        .transpose()?;

    let history = price_repo
        .get_price_history(asset_ids, start_date, end_date)
        .await
        .map_err(AppError::internal)?;

    let response = history
        .into_iter()
        .map(
            |(cmc_id, symbol, price_usd, timestamp)| AssetPriceHistoryDto {
                cmc_id,
                symbol,
                price_usd,
                timestamp: timestamp.to_string(),
            },
        )
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(response))
}
