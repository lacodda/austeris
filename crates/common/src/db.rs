//! The database pool, scoped to one service's schema.
//!
//! Services share a PostgreSQL instance and own a schema each (ADR 0001). The
//! schema is chosen by setting `search_path` on every connection through the
//! libpq `options` parameter. The 2025 compose file used `?currentSchema=`,
//! which is JDBC syntax — PostgreSQL ignores it, and every service was in fact
//! writing to `public`.

use anyhow::{Context, Result};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::config::Config;

/// Opens a pool whose every connection resolves unqualified names in `schema`.
///
/// The schema itself is created here, not by a migration: a migration runs
/// inside the schema it is supposed to create.
///
/// # Errors
///
/// Returns an error when the URL does not parse, the database is unreachable,
/// or the schema cannot be created.
pub async fn connect(config: &Config, schema: &str) -> Result<PgPool> {
    debug_assert!(is_bare_identifier(schema), "schema names are compile-time constants, not user input");

    let options: PgConnectOptions = config
        .database_url()?
        .parse::<PgConnectOptions>()
        .with_context(|| "AUSTERIS_DATABASE_URL is not a valid PostgreSQL connection string")?
        .options([("search_path", schema)]);

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(config.acquire_timeout)
        .connect_with(options)
        .await
        .with_context(|| format!("connecting to the database for schema `{schema}`"))?;

    // `search_path` points at a schema that need not exist yet; the first
    // migration would then fail with "no schema has been selected to create in".
    // Audited: `schema` is a crate-internal literal, checked above to be a bare
    // identifier. sqlx 0.9 requires the assertion to be explicit.
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\"")))
        .execute(&pool)
        .await
        .with_context(|| format!("creating schema `{schema}`"))?;

    Ok(pool)
}

/// Whether a name is safe to interpolate into DDL.
///
/// Schema names in this codebase are literals, never user input; this only
/// keeps a future careless caller from turning a literal into a variable.
fn is_bare_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::is_bare_identifier;

    #[test]
    fn accepts_the_schema_names_services_use() {
        assert!(is_bare_identifier("ledger"));
        assert!(is_bare_identifier("market_v2"));
    }

    #[test]
    fn rejects_anything_that_would_need_quoting() {
        assert!(!is_bare_identifier(""));
        assert!(!is_bare_identifier("Ledger"));
        assert!(!is_bare_identifier("ledger\"; drop schema public --"));
    }
}
