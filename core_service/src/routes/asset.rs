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
        (status = 200, description = "List of assets", body = Vec<AssetResponse>),
        (status = 500, description = "Internal server error")
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
    request_body = CreateAssetRequest,
    responses(
        (status = 200, description = "Asset created", body = AssetResponse),
        (status = 500, description = "Internal server error")
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
        (status = 200, description = "Assets updated"),
        (status = 500, description = "Internal server error")
    )
)]
async fn update_assets_handler(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<()> = update_assets(&pool).await;
    match result {
        Ok(()) => HttpResponse::Ok().json("Assets updated successfully"),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
