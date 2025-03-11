use crate::dto::wallet::{CreateWalletDto, WalletDto};
use crate::error::AppError;
use crate::services::wallet::WalletService;
use actix_web::{web, HttpResponse, Responder};
use actix_web_validator::Json;
use anyhow::Result;
use sqlx::PgPool;

// Configures routes for the /wallets scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/wallets")
            .route("", web::get().to(get_wallets))
            .route("", web::post().to(create_wallet)),
    );
}

// Handles GET /wallets to retrieve all wallets
#[utoipa::path(
    get,
    path = "/wallets",
    responses(
        (status = 200, description = "Successfully retrieved list of wallets", body = Vec<WalletDto>, example = json!([{"id": 1, "name": "Binance", "wallet_type": "Hot", "address": "0x1234", "created_at": "2024-01-01T00:00:00"}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Database connection failed"}))
    )
)]
async fn get_wallets(
    _pool: web::Data<PgPool>,
    wallet_service: web::Data<WalletService>,
) -> Result<impl Responder, AppError> {
    let wallets = wallet_service.get_all().await.map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(wallets))
}

// Handles POST /wallets to create a new wallet
#[utoipa::path(
    post,
    path = "/wallets",
    request_body(
        content = CreateWalletDto,
        description = "Details of the wallet to create",
        example = json!({"name": "Binance", "wallet_type": "Hot", "address": "0x1234"})
    ),
    responses(
        (status = 200, description = "Wallet created successfully", body = WalletDto, example = json!({"id": 1, "name": "Binance", "wallet_type": "Hot", "address": "0x1234", "created_at": "2024-01-01T00:00:00"})),
        (status = 400, description = "Invalid request data (e.g., missing required fields)", body = String, example = json!({"status": 400, "error": "Bad Request", "message": "Validation error: Name must not be empty"})),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to insert wallet into database"}))
    )
)]
async fn create_wallet(
    _pool: web::Data<PgPool>,
    wallet_service: web::Data<WalletService>,
    wallet: Json<CreateWalletDto>,
) -> Result<impl Responder, AppError> {
    let response = wallet_service
        .create(wallet.into_inner())
        .await
        .map_err(AppError::internal)?;
    Ok(HttpResponse::Ok().json(response))
}
