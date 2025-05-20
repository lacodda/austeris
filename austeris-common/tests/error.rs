// Tests for AppError
use austeris_common::error::AppError;
use actix_web::http::StatusCode;
use actix_web::ResponseError;
use actix_web_validator::Error as ValidatorError;

#[tokio::test]
async fn test_app_error_status_codes() {
    let internal_error = AppError::internal(anyhow::anyhow!("Test error"));
    assert_eq!(internal_error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);

    let bad_request = AppError::bad_request(anyhow::anyhow!("Invalid data"));
    assert_eq!(bad_request.status_code(), StatusCode::BAD_REQUEST);

    let service_unavailable = AppError::service_unavailable(anyhow::anyhow!("Service down"));
    assert_eq!(service_unavailable.status_code(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_app_error_response() {
    let error = AppError::bad_request(anyhow::anyhow!("Invalid input"));
    let response = error.error_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_validator_error_conversion() {
    let validator_error = ValidatorError::JsonPayloadError(
        actix_web::error::JsonPayloadError::Deserialize(
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid JSON",
            ))
        )
    );
    let app_error = AppError::from(validator_error);
    assert_eq!(app_error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
}
