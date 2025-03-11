use actix_web::{web, App, HttpServer};
use anyhow::Result;
use env_logger;
use log::LevelFilter;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

// Project modules
mod db;
mod dto;
mod error;
mod models;
mod repository;
mod routes;
mod services;

use db::connect;
use error::AppError;
use routes::{asset, snapshots, transaction, wallet};
use services::asset::AssetService;
use services::cmc::CmcService;
use services::portfolio::PortfolioService;
use services::redis::RedisService;
use services::wallet::WalletService;

#[derive(OpenApi)]
#[openapi(
    paths(
        asset::get_assets,
        asset::create_asset,
        asset::update_assets_handler,
        asset::get_asset_prices,
        asset::get_price_history,
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
            dto::asset::UpdateAssetsResponse,
            dto::asset::AssetPriceWithDetailsDto,
            dto::asset::AssetPriceHistoryDto,
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

#[actix_web::main]
async fn main() -> Result<()> {
    // Initialize logging with default level INFO
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .init();

    log::info!("Starting application on 127.0.0.1:9000");

    // Establish database connection
    let pool = connect().await?;

    // Initialize services
    let cmc_service = CmcService::new();
    let redis_service = RedisService::new()?;
    let asset_service = AssetService::new(web::Data::new(pool.clone()));
    let wallet_service = WalletService::new(web::Data::new(pool.clone()));
    let portfolio_service = PortfolioService::new(
        web::Data::new(pool.clone()),
        web::Data::new(cmc_service.clone()),
        web::Data::new(redis_service.clone()),
    );

    // Spawn periodic price updates
    let pool_for_task = pool.clone();
    let cmc_service_for_task = cmc_service.clone();
    let redis_service_for_task = redis_service.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(15 * 60)); // Every 15 minutes
        loop {
            interval.tick().await;
            log::info!("Updating asset prices...");
            match cmc_service_for_task
                .fetch_quotes_for_assets(&pool_for_task)
                .await
            {
                Ok(quotes) => {
                    let price_repo = repository::asset_price::AssetPriceRepository::new(
                        &pool_for_task,
                        redis_service_for_task.clone(),
                    );
                    match price_repo.save_prices(quotes).await {
                        Ok(count) => log::info!("Updated {} asset prices successfully", count),
                        Err(e) => log::error!("Failed to save prices: {}", e),
                    }
                }
                Err(e) => log::error!("Failed to fetch quotes from CMC: {}", e),
            }
        }
    });

    // Configure and start the HTTP server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(cmc_service.clone()))
            .app_data(web::Data::new(redis_service.clone()))
            .app_data(web::Data::new(asset_service.clone()))
            .app_data(web::Data::new(wallet_service.clone()))
            .app_data(web::Data::new(portfolio_service.clone()))
            .app_data(
                actix_web_validator::JsonConfig::default()
                    .error_handler(|err, _req| AppError::from(err).into()),
            )
            .app_data(
                actix_web_validator::QueryConfig::default()
                    .error_handler(|err, _req| AppError::from(err).into()),
            )
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
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
