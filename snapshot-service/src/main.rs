// Main entry point for Snapshot Service
use actix_web::{App, HttpServer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
    })
    .bind("0.0.0.0:8084")?
    .run()
    .await
}