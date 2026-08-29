// Main entry point for Asset Service
use actix_web::{web, App, HttpServer};
use austeris_common::{config::Config, db};
use sqlx::PgPool;
use tonic::transport::Server;
use routes::config;
use proto::asset_service_server::AssetServiceServer;
use services::AssetService;

mod routes;
mod services;
mod proto;
mod dto;
mod models;
mod repository;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logger
    env_logger::init();

    // Load configuration
    let config = Config::from_env().expect("Failed to load configuration");

    // Connect to database
    let pool = db::connect().await.expect("Failed to connect to database");

    // Connect to Redis
    let redis_client = redis::Client::open(config.redis_url.clone())
        .expect("Failed to connect to Redis");

    // Start gRPC server
    let grpc_addr = format!("0.0.0.0:{}", config.app_port + 1000).parse().unwrap();
    let asset_service = AssetService::new(pool.clone(), redis_client.clone());
    tokio::spawn(async move {
        Server::builder()
            .add_service(AssetServiceServer::new(asset_service))
            .serve(grpc_addr)
            .await
            .expect("Failed to start gRPC server");
    });

    // Start REST server
    let http_addr = format!("0.0.0.0:{}", config.app_port);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(redis_client.clone()))
            .configure(config)
    })
    .bind(http_addr)?
    .run()
    .await
}
