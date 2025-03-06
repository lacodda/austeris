use crate::models::wallet::{CreateWalletRequest, WalletRecord, WalletResponse};
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use sqlx::PgPool;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/wallets")
            .route("", web::get().to(get_wallets))
            .route("", web::post().to(create_wallet)),
    );
}

#[utoipa::path(
    get,
    path = "/wallets",
    responses(
        (status = 200, description = "Successfully retrieved list of wallets", body = Vec<WalletResponse>, example = json!([{"id": 1, "name": "Binance", "wallet_type": "Hot", "address": "0x1234", "created_at": "2024-01-01T00:00:00"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Database connection failed"))
    )
)]
async fn get_wallets(pool: web::Data<PgPool>) -> impl Responder {
    let result: Result<Vec<WalletResponse>> = (|| async {
        let wallets = sqlx::query_as!(
            WalletRecord,
            "SELECT id, name, type as wallet_type, address, created_at FROM wallets"
        )
        .fetch_all(pool.get_ref())
        .await?;

        let response = wallets
            .into_iter()
            .map(|record| WalletResponse {
                id: record.id,
                name: record.name,
                wallet_type: record.wallet_type,
                address: record.address,
                created_at: record.created_at.to_string(),
            })
            .collect::<Vec<_>>();

        Ok(response)
    })()
    .await;

    match result {
        Ok(wallets) => HttpResponse::Ok().json(wallets),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/wallets",
    request_body(
        content = CreateWalletRequest,
        description = "Details of the wallet to create",
        example = json!({"name": "Binance", "wallet_type": "Hot", "address": "0x1234"})
    ),
    responses(
        (status = 200, description = "Wallet created successfully", body = WalletResponse, example = json!({"id": 1, "name": "Binance", "wallet_type": "Hot", "address": "0x1234", "created_at": "2024-01-01T00:00:00"})),
        (status = 400, description = "Invalid request data (e.g., missing required fields)", body = String, example = json!("Validation error: name is required")),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!("Failed to insert wallet into database"))
    )
)]
async fn create_wallet(
    pool: web::Data<PgPool>,
    wallet: web::Json<CreateWalletRequest>,
) -> impl Responder {
    let result: Result<WalletResponse> = (|| async {
        let record = sqlx::query!(
            r#"
            INSERT INTO wallets (name, type, address)
            VALUES ($1, $2, $3)
            RETURNING id, name, type as wallet_type, address, created_at
            "#,
            wallet.name,
            wallet.wallet_type,
            wallet.address,
        )
        .fetch_one(pool.get_ref())
        .await?;

        Ok(WalletResponse {
            id: record.id,
            name: record.name,
            wallet_type: record.wallet_type,
            address: record.address,
            created_at: record.created_at.to_string(),
        })
    })()
    .await;

    match result {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
