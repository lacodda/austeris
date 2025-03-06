use crate::dto::transaction::{CreateTransactionDto, TransactionDto};
use crate::models::transaction::FilterParams;
use crate::repository::transaction::TransactionRepository;
use crate::services::portfolio::PortfolioService;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;
use log::error;
use sqlx::PgPool;

// Configures routes for the /transactions scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/transactions")
            .route("", web::get().to(get_transactions))
            .route("", web::post().to(create_transaction))
            .route("/portfolio/value", web::get().to(get_portfolio_value)),
    );
}

// Handles GET /transactions to retrieve filtered transactions
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
        // Use the repository to fetch transactions with filters
        let repo = TransactionRepository::new(pool.get_ref());
        let transactions = repo.get_transactions(query.into_inner()).await?;

        // Map database records to API response format (DTO)
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
            error!("Failed to get transactions: {}", e);
            // Handle specific error for invalid start_date format
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

// Handles POST /transactions to create a new transaction
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
        // Insert the transaction into the database and return its ID
        let record = sqlx::query!(
            r#"
            INSERT INTO transactions 
                (asset_id, wallet_id, amount, price, type, fee, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id
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
        Err(e) => {
            error!("Failed to create transaction: {}", e);
            HttpResponse::InternalServerError().json(e.to_string())
        }
    }
}

// Handles GET /transactions/portfolio/value to calculate portfolio value
#[utoipa::path(
    get,
    path = "/transactions/portfolio/value",
    responses(
        (status = 200, description = "Successfully calculated portfolio value in USD", body = serde_json::Value, example = json!({"total_value_usd": 25000.0})),
        (status = 500, description = "Internal server error (e.g., database or CoinMarketCap API failure)", body = String, example = json!("Failed to fetch price data from CoinMarketCap")),
        (status = 503, description = "Price data unavailable for some assets", body = String, example = json!("Price unavailable for symbol BTC"))
    )
)]
async fn get_portfolio_value(portfolio: web::Data<PortfolioService>) -> impl Responder {
    let result: Result<f64> = portfolio.get_portfolio_value().await;
    match result {
        Ok(total_value) => {
            HttpResponse::Ok().json(serde_json::json!({"total_value_usd": total_value}))
        }
        Err(e) => {
            error!("Failed to calculate portfolio value: {}", e);
            HttpResponse::InternalServerError().json(e.to_string())
        }
    }
}
