use crate::dto::snapshot::SnapshotDto;
use crate::error::AppError;
use crate::services::snapshot::SnapshotService;
use actix_web::{web, HttpResponse, Responder};
use anyhow::Result;

// Configures routes for the /snapshots scope
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/snapshots")
            .route("", web::post().to(create_snapshot))
            .route("", web::get().to(get_snapshots)),
    );
}

// Handles POST /snapshots to create a new portfolio snapshot
#[utoipa::path(
    post,
    path = "/snapshots",
    responses(
        (status = 200, description = "Snapshot created successfully", body = SnapshotDto, example = json!({"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": 1}, {"symbol": "ETH", "amount": 10.0, "cmc_id": 1027}]})),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to save snapshot to database"}))
    )
)]
async fn create_snapshot(
    snapshot_service: web::Data<SnapshotService>,
) -> Result<impl Responder, AppError> {
    let response = snapshot_service.create().await?;
    Ok(HttpResponse::Ok().json(response))
}

// Handles GET /snapshots to retrieve all snapshots with differences
#[utoipa::path(
    get,
    path = "/snapshots",
    responses(
        (status = 200, description = "Successfully retrieved list of snapshots with differences", body = Vec<SnapshotDto>, example = json!([{"id": 1, "created_at": "2025-03-06T14:00:00", "assets": [{"symbol": "BTC", "amount": 1.5, "cmc_id": 1}, {"symbol": "ETH", "amount": 10.0, "cmc_id": 1027}], "diff": [{"symbol": "BTC", "amount_diff": -0.5, "cmc_id": 1}, {"symbol": "ETH", "amount_diff": 2.0, "cmc_id": 1027}]}])),
        (status = 500, description = "Internal server error (e.g., database failure)", body = String, example = json!({"status": 500, "error": "Internal Server Error", "message": "Failed to fetch snapshots from database"}))
    )
)]
async fn get_snapshots(
    snapshot_service: web::Data<SnapshotService>,
) -> Result<impl Responder, AppError> {
    let snapshots = snapshot_service.get_all().await?;
    Ok(HttpResponse::Ok().json(snapshots))
}
