use crate::dto::asset::{AssetDto, CreateAssetDto};
use crate::error::AppError;
use crate::models::asset::AssetDb;
use crate::services::cmc::update_assets;
use actix_web::{web, HttpResponse, Responder};
use actix_web_validator::Json;
use anyhow::Result;
use sqlx::PgPool;

// Configures routes for the /assets scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assets")
            .route("", web::get().to(get_assets))
            .route("", web::post().to(create_asset))
            .route("/update", web::post().to(update_assets_handler)),
    );
}

// Handles GET /assets to retrieve all assets
#[utoipa::path(
    get,
    path = "/assets",
    responses(
        (status = 200, description = "Successfully retrieved list of assets", body = Vec<AssetDto>, example = json!([{"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8, "created_at": "2024-01-01T00:00:00"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Database connection failed"}))
    )
)]
async fn get_assets(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    // Fetch all assets from the database
    let assets = sqlx::query_as!(
        AssetDb,
        r#"
        SELECT id, symbol, name, cmc_id, decimals, created_at
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
        example = json!({"symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8})
    ),
    responses(
        (status = 200, description = "Asset created successfully", body = AssetDto, example = json!({"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8, "created_at": "2024-01-01T00:00:00"})),
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
        INSERT INTO assets (symbol, name, cmc_id, decimals)
        VALUES ($1, $2, $3, $4)
        RETURNING id, symbol, name, cmc_id, decimals, created_at
        "#,
        asset.symbol,
        asset.name,
        asset.cmc_id,
        asset.decimals,
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
        created_at: record.created_at.to_string(),
    };

    Ok(HttpResponse::Ok().json(response))
}

// Handles POST /assets/update to sync assets with CoinMarketCap
#[utoipa::path(
    post,
    path = "/assets/update",
    responses(
        (status = 200, description = "Assets updated successfully from CoinMarketCap", body = String, example = json!("Assets updated successfully")),
        (status = 500, description = "Internal server error (e.g., CoinMarketCap API or database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to fetch listings from CoinMarketCap"}))
    )
)]
async fn update_assets_handler(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    update_assets(&pool).await.map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json("Assets updated successfully"))
}
