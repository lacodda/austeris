use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use actix_web_validator::Error as ValidatorError;
use serde::Serialize;
use std::fmt;

// Custom application error type wrapping anyhow::Error
#[derive(Debug)]
pub struct AppError {
    inner: anyhow::Error,
    status: StatusCode,
}

impl AppError {
    // Creates a new AppError with a specific status code
    pub fn new(err: anyhow::Error, status: StatusCode) -> Self {
        log::error!("Error occurred: {}", err); // Log the error
        Self { inner: err, status }
    }

    // Convenience method for internal server errors
    pub fn internal(err: impl Into<anyhow::Error>) -> Self {
        Self::new(err.into(), StatusCode::INTERNAL_SERVER_ERROR)
    }

    // Convenience method for bad request errors
    pub fn bad_request(err: impl Into<anyhow::Error>) -> Self {
        Self::new(err.into(), StatusCode::BAD_REQUEST)
    }

    // Convenience method for service unavailable errors
    pub fn service_unavailable(err: impl Into<anyhow::Error>) -> Self {
        Self::new(err.into(), StatusCode::SERVICE_UNAVAILABLE)
    }
}

// Structure for JSON error response
#[derive(Serialize)]
struct ErrorResponse {
    status: u16,
    error: String,
    message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err)
    }
}

impl From<ValidatorError> for AppError {
    fn from(err: ValidatorError) -> Self {
        match err {
            ValidatorError::Validate(e) => {
                let message = e
                    .field_errors()
                    .iter()
                    .flat_map(|(field, errors)| {
                        errors.iter().map(move |error| {
                            format!(
                                "{}: {}",
                                field,
                                error
                                    .message
                                    .as_ref()
                                    .unwrap_or(&"Validation failed".into())
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Self::bad_request(anyhow::anyhow!("Validation error: {}", message))
            }
            _ => Self::internal(anyhow::anyhow!("Unexpected validation error: {}", err)),
        }
    }
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        let response = ErrorResponse {
            status: self.status.as_u16(),
            error: self
                .status
                .canonical_reason()
                .unwrap_or("Unknown")
                .to_string(),
            message: self.inner.to_string(),
        };
        HttpResponse::build(self.status).json(response)
    }
}
