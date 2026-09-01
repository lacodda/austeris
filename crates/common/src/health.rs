//! The two endpoints an orchestrator polls, and the difference between them.
//!
//! `/healthz` answers "is this process alive" — it touches nothing, so a
//! restart loop cannot be triggered by a database hiccup. `/readyz` answers
//! "can this process serve requests": the database answers, and its schema is
//! migrated to the version this binary was built against. Compose waits on
//! `/readyz` (`condition: service_healthy`), which is what keeps a service from
//! starting against a schema it does not understand.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// What a service needs to answer a readiness probe.
#[derive(Debug, Clone)]
pub struct Readiness {
    pool: PgPool,
    migrator: &'static sqlx::migrate::Migrator,
}

impl Readiness {
    /// Binds a probe to one service's pool and its embedded migrations.
    #[must_use]
    pub fn new(pool: PgPool, migrator: &'static sqlx::migrate::Migrator) -> Self {
        Self { pool, migrator }
    }
}

/// The body both probes answer with.
#[derive(Debug, Serialize, Deserialize)]
pub struct Health {
    /// `ok` when the check passed, otherwise what failed.
    pub status: String,
    /// The last migration this binary carries; `None` when it carries none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<i64>,
}

/// Mounts `/healthz` and `/readyz` on a router.
///
/// A service with no database of its own passes `None` and gets a `/readyz`
/// that is as cheap as `/healthz` — still present, so the compose file does not
/// need a special case per service.
pub fn routes<S: Clone + Send + Sync + 'static>(readiness: Option<Readiness>) -> Router<S> {
    let ready = match readiness {
        Some(readiness) => get(readyz).with_state(readiness),
        None => get(|| async {
            Health {
                status: "ok".to_owned(),
                schema_version: None,
            }
            .into_response()
        }),
    };
    Router::new().route("/healthz", get(healthz)).route("/readyz", ready)
}

async fn healthz() -> Response {
    Health {
        status: "ok".to_owned(),
        schema_version: None,
    }
    .into_response()
}

async fn readyz(State(readiness): State<Readiness>) -> Response {
    match check(&readiness).await {
        Ok(version) => Health {
            status: "ok".to_owned(),
            schema_version: version,
        }
        .into_response(),
        Err(reason) => {
            tracing::warn!(%reason, "not ready");
            let body = Health {
                status: reason,
                schema_version: None,
            };
            (StatusCode::SERVICE_UNAVAILABLE, axum::Json(body)).into_response()
        }
    }
}

/// Returns the applied schema version, or why the service is not ready.
async fn check(readiness: &Readiness) -> Result<Option<i64>, String> {
    let mut connection = readiness.pool.acquire().await.map_err(|e| format!("database unavailable: {e}"))?;

    let expected = readiness.migrator.iter().map(|m| m.version).max();
    let Some(expected) = expected else {
        // No migrations to be behind on: reaching the database is the whole check.
        return Ok(None);
    };

    // `applied` is the newest migration in *this schema*, because the pool's
    // `search_path` puts the bookkeeping table inside it. Two services are
    // never compared against each other's schema version. The table's name
    // comes from the migrator rather than a literal, so a service that moves
    // it does not silently get a probe reading a table that is not there.
    let sql = format!("SELECT MAX(version) FROM \"{}\" WHERE success", readiness.migrator.table_name);
    let applied: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&mut *connection)
        .await
        .map_err(|e| format!("schema not migrated: {e}"))?;

    match applied {
        Some(applied) if applied >= expected => Ok(Some(applied)),
        Some(applied) => Err(format!("schema is at version {applied}, this build needs {expected}")),
        None => Err(format!("schema is empty, this build needs version {expected}")),
    }
}

impl IntoResponse for Health {
    fn into_response(self) -> Response {
        axum::Json(self).into_response()
    }
}
