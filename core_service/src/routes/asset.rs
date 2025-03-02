use crate::models::asset::{AssetRecord, AssetResponse, CreateAssetRequest};
use actix_web::{web, HttpResponse, Responder};
use sqlx::PgPool;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assets")
            .route("", web::get().to(get_assets))
            .route("", web::post().to(create_asset)),
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
    let assets = sqlx::query_as!(
        AssetRecord,
        r#"
        SELECT id, symbol, name, cmc_id, decimals, created_at
        FROM assets
        "#
    )
    .fetch_all(pool.get_ref())
    .await;

    match assets {
        Ok(records) => {
            let assets = records
                .into_iter()
                .map(|record| AssetResponse {
                    id: record.id,
                    symbol: record.symbol,
                    name: record.name,
                    cmc_id: record.cmc_id,
                    decimals: record.decimals,
                    created_at: record.created_at.to_string(), // Преобразуем в строку
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(assets)
        }
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
    let result = sqlx::query!(
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
    .await;

    match result {
        Ok(record) => HttpResponse::Ok().json(AssetResponse {
            id: record.id,
            symbol: record.symbol,
            name: record.name,
            cmc_id: record.cmc_id,
            decimals: record.decimals,
            created_at: record.created_at.to_string(),
        }),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
