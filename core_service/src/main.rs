use actix_web::{web, App, HttpServer};
use anyhow::Result;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod db;
mod models;
mod routes;
mod services;

use db::connect;
use routes::{asset, snapshots, transaction, wallet};

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
        snapshots::create_snapshot
    ),
    components(
        schemas(
            models::asset::AssetResponse,
            models::asset::CreateAssetRequest,
            models::wallet::WalletResponse,
            models::wallet::CreateWalletRequest,
            models::transaction::TransactionResponse,
            models::transaction::CreateTransactionRequest,
            models::snapshot::PortfolioSnapshot,
            models::snapshot::SnapshotAsset,
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
    let pool = connect().await?;
    let cmc_service = services::cmc::CmcService::new();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(cmc_service.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            .configure(asset::configure)
            .configure(wallet::configure)
            .configure(transaction::configure)
            .configure(snapshots::configure) // Добавляем
    })
    .bind("127.0.0.1:9000")?
    .run()
    .await?;
    Ok(())
}
