use crate::models::asset::{AssetRecord, AssetResponse, CreateAssetRequest};
use crate::services::cmc::update_assets;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use sqlx::PgPool;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assets")
            .route("", web::get().to(get_assets))
            .route("", web::post().to(create_asset))
            .route("/update", web::post().to(update_assets_handler)),
    );
}

#[utoipa::path(
    get,
    path = "/assets",
    responses(
        (status = 200, description = "Successfully retrieved list of assets", body = Vec<AssetResponse>, example = json!([{"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8, "created_at": "2024-01-01T00:00:00"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Database connection failed"))
    )
)]
async fn get_assets(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<Vec<AssetResponse>> = (|| async {
        let assets = sqlx::query_as!(
            AssetRecord,
            r#"
            SELECT id, symbol, name, cmc_id, decimals, created_at
            FROM assets
            "#
        )
        .fetch_all(pool.get_ref())
        .await?;

        let response = assets
            .into_iter()
            .map(|record| AssetResponse {
                id: record.id,
                symbol: record.symbol,
                name: record.name,
                cmc_id: record.cmc_id,
                decimals: record.decimals,
                created_at: record.created_at.to_string(),
            })
            .collect::<Vec<_>>();

        Ok(response)
    })()
    .await;

    match result {
        Ok(assets) => HttpResponse::Ok().json(assets),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/assets",
    request_body(
        content = CreateAssetRequest,
        description = "Details of the asset to create",
        example = json!({"symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8})
    ),
    responses(
        (status = 200, description = "Asset created successfully", body = AssetResponse, example = json!({"id": 1, "symbol": "BTC", "name": "Bitcoin", "cmc_id": "1", "decimals": 8, "created_at": "2024-01-01T00:00:00"})),
        (status = 400, description = "Invalid request data (e.g., missing required fields)", body = String, example = json!("Validation error: symbol is required")),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to insert asset into database"))
    )
)]
async fn create_asset(
    pool: web::Data<PgPool>,
    asset: web::Json<CreateAssetRequest>,
) -> impl Responder {
    let result: Result<AssetResponse> = (|| async {
        let record = sqlx::query!(
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
        .await?;

        Ok(AssetResponse {
            id: record.id,
            symbol: record.symbol,
            name: record.name,
            cmc_id: record.cmc_id,
            decimals: record.decimals,
            created_at: record.created_at.to_string(),
        })
    })()
    .await;

    match result {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/assets/update",
    responses(
        (status = 200, description = "Assets updated successfully from CoinMarketCap", body = String, example = json!("Assets updated successfully")),
        (status = 500, description = "Internal server error (e.g., CoinMarketCap API or database failure)", body = String, example = json!("Failed to fetch listings from CoinMarketCap"))
    )
)]
async fn update_assets_handler(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<()> = update_assets(&pool).await;
    match result {
        Ok(()) => HttpResponse::Ok().json("Assets updated successfully"),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
