use crate::dto::transaction::{CreateTransactionDto, TransactionDto};
use crate::models::transaction::FilterParams;
use crate::repository::transaction::TransactionRepository;
use crate::services::cmc::CmcService;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use sqlx::PgPool;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/transactions")
            .route("", web::get().to(get_transactions))
            .route("", web::post().to(create_transaction))
            .route("/portfolio/value", web::get().to(get_portfolio_value)),
    );
}

#[utoipa::path(
    get,
    path = "/transactions",
    responses(
        (status = 200, description = "Successfully retrieved list of transactions", body = Vec<TransactionDto>, example = json!([{"id": 1, "asset": "BTC", "wallet": "Binance", "amount": 0.5, "price": 50000.0, "transaction_type": "BUY", "fee": 0.001, "notes": "First trade", "created_at": "2024-01-01T00:00:00"}])),
        (status = 400, description = "Invalid start_date format", body = String, example = json!("Invalid start_date format, expected ISO 8601 (e.g., '2024-01-01T00:00:00')")),
        (status = 500, description = "Internal server error", body = String, example = json!("Database connection failed"))
    ),
    params(
        ("asset_id" = Option<i32>, Query, description = "Filter transactions by asset ID (e.g., 1 for BTC)"),
        ("wallet_id" = Option<i32>, Query, description = "Filter transactions by wallet ID (e.g., 1 for Binance)"),
        ("start_date" = Option<String>, Query, description = "Filter transactions starting from this date in ISO 8601 format (e.g., '2024-01-01T00:00:00')"),
        ("limit" = Option<i64>, Query, description = "Maximum number of transactions to return (default: 10)"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination (default: 0)")
    )
)]
async fn get_transactions(
    pool: web::Data<PgPool>,
    query: web::Query<FilterParams>,
) -> impl Responder {
    let result: Result<Vec<TransactionDto>> = (|| async {
        let repo = TransactionRepository::new(pool.get_ref());
        let transactions = repo.get_transactions(query.into_inner()).await?;

        let response = transactions
            .into_iter()
            .map(|record| TransactionDto {
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

        Ok(response)
    })()
    .await;

    match result {
        Ok(transactions) => HttpResponse::Ok().json(transactions),
        Err(e) => {
            if e.to_string().contains("Invalid start_date format") {
                HttpResponse::BadRequest().json(
                    "Invalid start_date format, expected ISO 8601 (e.g., '2024-01-01T00:00:00')",
                )
            } else {
                HttpResponse::InternalServerError().json(e.to_string())
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/transactions",
    request_body(
        content = CreateTransactionDto,
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
    transaction: web::Json<CreateTransactionDto>,
) -> impl Responder {
    let result: Result<i32> = (|| async {
        let record = sqlx::query!(
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
        .await?;
        Ok(record.id)
    })()
    .await;

    match result {
        Ok(id) => HttpResponse::Ok().json(id),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    get,
    path = "/transactions/portfolio/value",
    responses(
        (status = 200, description = "Successfully calculated portfolio value in USD", body = serde_json::Value, example = json!({"total_value_usd": 25000.0})),
        (status = 500, description = "Internal server error (e.g., database or CoinMarketCap API failure)", body = String, example = json!("Failed to fetch price data from CoinMarketCap")),
        (status = 503, description = "Price data unavailable for some assets", body = String, example = json!("Price unavailable for symbol BTC"))
    )
)]
async fn get_portfolio_value(
    pool: web::Data<PgPool>,
    cmc: web::Data<CmcService>,
) -> impl Responder {
    let result: Result<f64> = (|| async {
        let repo = TransactionRepository::new(pool.get_ref());
        let transactions = repo.get_all_transactions().await?;

        let mut total_value = 0.0;
        let mut asset_amounts: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();

        for record in transactions {
            let amount = asset_amounts.entry(record.asset.clone()).or_insert(0.0);
            if record.transaction_type == "BUY" {
                *amount += record.amount;
            } else if record.transaction_type == "SELL" {
                *amount -= record.amount;
            }
        }

        for (symbol, amount) in asset_amounts {
            if amount > 0.0 {
                let quote = cmc.get_quote(&symbol).await?;
                if let Some(price) = quote.price {
                    total_value += amount * price;
                }
            }
        }

        Ok(total_value)
    })()
    .await;

    match result {
        Ok(total_value) => {
            HttpResponse::Ok().json(serde_json::json!({"total_value_usd": total_value}))
        }
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}
