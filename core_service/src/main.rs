use actix_web::{web, App, HttpServer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod db;
mod models;
mod routes;

use db::connect;
use routes::{asset, transaction, wallet};

#[derive(OpenApi)]
#[openapi(
    paths(
        asset::get_assets,
        asset::create_asset,
        wallet::get_wallets,
        wallet::create_wallet,
        transaction::get_transactions,
        transaction::create_transaction
    ),
    components(
        schemas(
            models::asset::AssetResponse,
            models::asset::CreateAssetRequest,
            models::wallet::WalletResponse,
            models::wallet::CreateWalletRequest,
            models::transaction::TransactionResponse,
            models::transaction::CreateTransactionRequest
        )
    ),
    tags(
        (name = "Assets", description = "Aasset management"),
        (name = "Wallets", description = "Wallet management"),
        (name = "Transactions", description = "Transaction management")
    )
)]
struct ApiDoc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let pool = connect().await.expect("Failed to connect to database");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
            .configure(asset::configure)
            .configure(wallet::configure)
            .configure(transaction::configure)
    })
    .bind("127.0.0.1:9000")?
    .run()
    .await
}
