//! The identity service's gRPC surface.
//!
//! One call: hand over a session token, get back who holds it. This is how
//! every other service learns who is calling without any of them ever seeing a
//! password (ADR 0001, ADR 0003).

use austeris_proto::identity::v1::identity_service_server::{IdentityService, IdentityServiceServer};
use austeris_proto::identity::v1::{ValidateSessionRequest, ValidateSessionResponse};
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::session;

/// The service implementation.
pub struct Service {
    pool: PgPool,
}

impl Service {
    /// Wraps a pool as a gRPC service ready to be served.
    #[must_use]
    pub fn new(pool: PgPool) -> IdentityServiceServer<Self> {
        IdentityServiceServer::new(Self { pool })
    }
}

#[tonic::async_trait]
impl IdentityService for Service {
    async fn validate_session(&self, request: Request<ValidateSessionRequest>) -> Result<Response<ValidateSessionResponse>, Status> {
        let token = request.into_inner().token;

        // An empty answer rather than an error: an unknown token, an expired
        // session and a deactivated account are three cases the caller has no
        // business telling apart, and none of them is a failure of this call.
        let holder = session::authenticate(&self.pool, &token).await.map_err(|error| {
            tracing::error!(%error, "validating a session failed");
            Status::internal("could not validate the session")
        })?;

        Ok(Response::new(match holder {
            Some(holder) => ValidateSessionResponse {
                user_id: holder.user_id.to_string(),
                email: holder.email,
            },
            None => ValidateSessionResponse::default(),
        }))
    }
}
