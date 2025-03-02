use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};
use sqlx::types::time::PrimitiveDateTime;
use sqlx::PgPool;
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

mod db;
use db::connect;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct Transaction {
    asset: String,
    amount: f64,
    price: f64,
    transaction_type: String,
}

#[derive(Debug, Serialize)]
struct TransactionRecord {
    asset: Option<String>,
    amount: Option<f64>,
    price: Option<f64>,
    transaction_type: Option<String>,
    created_at: Option<NaiveDateTime>,
}

#[derive(Debug)]
struct TransactionRecordRaw {
    asset: Option<String>,
    amount: Option<f64>,
    price: Option<f64>,
    transaction_type: Option<String>,
    created_at: Option<PrimitiveDateTime>,
}

#[derive(OpenApi)]
#[openapi(
    paths(create_transaction, get_portfolio),
    components(schemas(Transaction))
)]
struct ApiDoc;

#[utoipa::path(
    post,
    path = "/transactions",
    request_body = Transaction,
    responses(
        (status = 200, description = "Transaction created"),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_transaction(
    transaction: web::Json<Transaction>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    let result = sqlx::query!(
        r#"
        INSERT INTO transactions (asset, amount, price, type)
        VALUES ($1, $2, $3, $4)
        "#,
        transaction.asset,
        transaction.amount,
        transaction.price,
        transaction.transaction_type
    )
    .execute(pool.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().json("Transaction created"),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[utoipa::path(
    get,
    path = "/portfolio",
    responses(
        (status = 200, description = "List of transactions"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_portfolio(pool: web::Data<PgPool>) -> impl Responder {
    let transactions = sqlx::query_as!(
        TransactionRecordRaw,
        r#"
        SELECT asset, amount, price, type as transaction_type, created_at
        FROM transactions
        "#
    )
    .fetch_all(pool.get_ref())
    .await;

    match transactions {
        Ok(rows) => {
            let transactions = rows
                .into_iter()
                .map(|record| TransactionRecord {
                    asset: record.asset,
                    amount: record.amount,
                    price: record.price,
                    transaction_type: record.transaction_type,
                    created_at: record.created_at.map(|dt| {
                        DateTime::from_timestamp(dt.assume_utc().unix_timestamp(), 0)
                            .unwrap()
                            .naive_utc()
                    }),
                })
                .collect::<Vec<_>>();
            HttpResponse::Ok().json(transactions)
        }
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let pool = connect().await.expect("Failed to connect to database");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .service(web::resource("/transactions").route(web::post().to(create_transaction)))
            .service(web::resource("/portfolio").route(web::get().to(get_portfolio)))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
    })
    .bind("127.0.0.1:9000")?
    .run()
    .await
}
