use crate::models::transaction::{
    CreateTransactionRequest, FilterParams, TransactionRecord, TransactionResponse,
};
use actix_web::{web, HttpResponse, Responder};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::PgPool;
use time::format_description::well_known::Iso8601;

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
        (status = 200, description = "Successfully retrieved list of transactions", body = Vec<TransactionResponse>, example = json!([{"id": 1, "asset": "BTC", "wallet": "Binance", "amount": 0.5, "price": 50000.0, "transaction_type": "BUY", "fee": 0.001, "notes": "First trade", "created_at": "2024-01-01T00:00:00"}])),
        (status = 400, description = "Invalid start_date format", body = String, example = json!("Invalid start_date format, expected ISO 8601 (e.g., '2024-01-01T00:00:00')")),
        (status = 500, description = "Internal server error", body = String, example = json!("Database connection failed"))
    ),
    params(
        ("asset_id" = Option<i32>, Query, description = "Filter transactions by asset ID (e.g., 1 for BTC)"),
        ("wallet_id" = Option<i32>, Query, description = "Filter transactions by wallet ID (e.g., 1 for Binance)"),
        ("start_date" = Option<String>, Query, description = "Filter transactions starting from this date in ISO 8601 format (e.g., '2024-01-01T00:00:00')"),
        ("limit" = Option<i64>, Query, description = "Maximum number of transactions to return (default: 10)"),
        ("offset" = Option<i64>, Query, description = "Number of transactions to skip for pagination (default: 0)")
    )
)]
async fn get_transactions(
    pool: web::Data<PgPool>,
    query: web::Query<FilterParams>,
) -> impl Responder {
    let mut sql = String::from(
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
        WHERE 1=1
        "#,
    );

    if let Some(asset_id) = query.asset_id {
        sql.push_str(&format!(" AND t.asset_id = {}", asset_id));
    }
    if let Some(wallet_id) = query.wallet_id {
        sql.push_str(&format!(" AND t.wallet_id = {}", wallet_id));
    }
    if let Some(start_date) = &query.start_date {
        match PrimitiveDateTime::parse(start_date, &Iso8601::DEFAULT) {
            Ok(parsed_date) => sql.push_str(&format!(" AND t.created_at >= '{}'", parsed_date)),
            Err(_) => {
                return HttpResponse::BadRequest().json(
                    "Invalid start_date format, expected ISO 8601 (e.g., '2024-01-01T00:00:00')",
                )
            }
        }
    }
    sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        query.limit.unwrap_or(10),
        query.offset.unwrap_or(0)
    ));

    let transactions = sqlx::query_as::<_, TransactionRecord>(&sql)
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
                    created_at: record.created_at.to_string(),
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
    request_body(
        content = CreateTransactionRequest,
        description = "Details of the transaction to create",
        example = json!({"asset_id": 1, "wallet_id": 1, "amount": 0.5, "price": 50000.0, "transaction_type": "BUY", "fee": 0.001, "notes": "First trade"})
    ),
    responses(
        (status = 200, description = "Transaction created successfully", body = i32, example = json!(1)),
        (status = 400, description = "Invalid request data (e.g., validation failed)", body = String, example = json!("Validation error: amount must be >= 0")),
        (status = 500, description = "Internal server error", body = String, example = json!("Failed to insert transaction into database"))
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
