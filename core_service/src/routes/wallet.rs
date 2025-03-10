use crate::dto::wallet::{CreateWalletDto, WalletDto};
use crate::error::AppError;
use crate::models::wallet::WalletDb;
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
async fn get_wallets(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    // Fetch all wallets from the database
    let wallets = sqlx::query_as!(
        WalletDb,
        "SELECT id, name, type as wallet_type, address, created_at FROM wallets"
    )
    .fetch_all(pool.get_ref())
    .await
    .map_err(AppError::internal)?;

    // Map database records to API response format (DTO)
    let response = wallets
        .into_iter()
        .map(|record| WalletDto {
            id: record.id,
            name: record.name,
            wallet_type: record.wallet_type,
            address: record.address,
            created_at: record.created_at.to_string(),
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(response))
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
    pool: web::Data<PgPool>,
    wallet: Json<CreateWalletDto>,
) -> Result<impl Responder, AppError> {
    // Insert the new wallet into the database and return its details
    let record = sqlx::query_as!(
        WalletDb,
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
    .await
    .map_err(AppError::internal)?;

    let response = WalletDto {
        id: record.id,
        name: record.name,
        wallet_type: record.wallet_type,
        address: record.address,
        created_at: record.created_at.to_string(),
    };

    Ok(HttpResponse::Ok().json(response))
}
