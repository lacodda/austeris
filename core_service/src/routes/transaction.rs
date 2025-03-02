use crate::models::transaction::{
    CreateTransactionRequest, TransactionRecord, TransactionResponse,
};
use actix_web::{web, HttpResponse, Responder};
use sqlx::PgPool;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/transactions")
            .route("", web::get().to(get_transactions))
            .route("", web::post().to(create_transaction)),
    );
}

#[utoipa::path(
    get,
    path = "/transactions",
    responses(
        (status = 200, description = "List of transactions", body = Vec<TransactionResponse>),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_transactions(pool: web::Data<PgPool>) -> impl Responder {
    let transactions = sqlx::query_as!(
        TransactionRecord,
        r#"
        SELECT 
            t.id, 
            a.symbol as asset, 
            w.name as wallet,
            t.amount,
            t.price,
            t.type as transaction_type,
            t.fee,
            t.notes,
            t.created_at
        FROM transactions t
        JOIN assets a ON t.asset_id = a.id
        JOIN wallets w ON t.wallet_id = w.id
        "#
    )
    .fetch_all(pool.get_ref())
    .await;

    match transactions {
        Ok(records) => {
            let transactions = records
                .into_iter()
                .map(|record| TransactionResponse {
                    id: record.id,
                    asset: record.asset,
                    wallet: record.wallet,
                    amount: record.amount,
                    price: record.price,
                    transaction_type: record.transaction_type,
                    fee: record.fee,
                    notes: record.notes,
                    created_at: record.created_at.to_string(), // Преобразуем в строку
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(transactions)
        }
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/transactions",
    request_body = CreateTransactionRequest,
    responses(
        (status = 200, description = "Transaction created", body = TransactionResponse),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_transaction(
    pool: web::Data<PgPool>,
    transaction: web::Json<CreateTransactionRequest>,
) -> impl Responder {
    let result = sqlx::query!(
        r#"
        INSERT INTO transactions 
            (asset_id, wallet_id, amount, price, type, fee, notes)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, created_at
        "#,
        transaction.asset_id,
        transaction.wallet_id,
        transaction.amount,
        transaction.price,
        transaction.transaction_type,
        transaction.fee,
        transaction.notes,
    )
    .fetch_one(pool.get_ref())
    .await;

    match result {
        Ok(record) => HttpResponse::Ok().json(record.id),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
