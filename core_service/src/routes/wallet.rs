use crate::models::wallet::{CreateWalletRequest, WalletRecord, WalletResponse};
use actix_web::{web, HttpResponse, Responder};
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
        (status = 200, description = "List of wallets", body = Vec<WalletResponse>),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_wallets(pool: web::Data<PgPool>) -> impl Responder {
    let wallets = sqlx::query_as!(
        WalletRecord,
        "SELECT id, name, type as wallet_type, address, created_at FROM wallets"
    )
    .fetch_all(pool.get_ref())
    .await;

    match wallets {
        Ok(records) => {
            let wallets = records
                .into_iter()
                .map(|record| WalletResponse {
                    id: record.id,
                    name: record.name,
                    wallet_type: record.wallet_type,
                    address: record.address,
                    created_at: record.created_at.to_string(), // Преобразуем в строку
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(wallets)
        }
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/wallets",
    request_body = CreateWalletRequest,
    responses(
        (status = 200, description = "Wallet created", body = WalletResponse),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_wallet(
    pool: web::Data<PgPool>,
    wallet: web::Json<CreateWalletRequest>,
) -> impl Responder {
    let result = sqlx::query!(
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
    .await;

    match result {
        Ok(record) => HttpResponse::Ok().json(WalletResponse {
            id: record.id,
            name: record.name,
            wallet_type: record.wallet_type,
            address: record.address,
            created_at: record.created_at.to_string(),
        }),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
