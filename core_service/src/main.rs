use actix_web::{web, App, HttpServer};
use anyhow::Result;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

// Project modules
mod db;
mod dto;
mod models;
mod repository;
mod routes;
mod services;

use db::connect;
use routes::{asset, snapshots, transaction, wallet};

// Define OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(
    paths(
        asset::get_assets,
        asset::create_asset,
        asset::update_assets_handler,
        wallet::get_wallets,
        wallet::create_wallet,
        transaction::get_transactions,
        transaction::create_transaction,
        transaction::get_portfolio_value,
        snapshots::create_snapshot,
        snapshots::get_snapshots
    ),
    components(
        schemas(
            dto::asset::AssetDto,
            dto::asset::CreateAssetDto,
            dto::wallet::WalletDto,
            dto::wallet::CreateWalletDto,
            dto::transaction::TransactionDto,
            dto::transaction::CreateTransactionDto,
            dto::snapshot::SnapshotDto,
            dto::snapshot::SnapshotAssetDto,
            dto::snapshot::SnapshotDiffDto
        )
    ),
    tags(
        (name = "Assets", description = "Asset management"),
        (name = "Wallets", description = "Wallet management"),
        (name = "Transactions", description = "Transaction management"),
        (name = "Snapshots", description = "Portfolio snapshot management")
    )
)]
struct ApiDoc;

// Main entry point for the application
#[actix_web::main]
async fn main() -> Result<()> {
    // Establish database connection
    let pool = connect().await?;

    // Initialize CoinMarketCap service
    let cmc_service = services::cmc::CmcService::new();

    // Configure and start the HTTP server
    HttpServer::new(move || {
        App::new()
            // Share database pool and CMC service across requests
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(cmc_service.clone()))
            // Set up Swagger UI endpoint
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            // Configure route handlers
            .configure(asset::configure)
            .configure(wallet::configure)
            .configure(transaction::configure)
            .configure(snapshots::configure)
    })
    .bind("127.0.0.1:9000")?
    .run()
    .await?;

    Ok(())
}
