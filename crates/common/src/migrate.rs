//! Applying and rolling back one service's schema.
//!
//! Each service owns its migrations and its schema (ADR 0001), so a migration
//! run is always scoped to one service: the pool's `search_path` puts both the
//! tables and sqlx's own `_sqlx_migrations` bookkeeping inside that schema, and
//! two services can never see each other's version.
//!
//! Migrations are reversible — every `.up.sql` has a `.down.sql` — so a release
//! that turns out wrong is undone by migrating down, not by restoring the
//! backup and losing everything written since.

use anyhow::{Context, Result, bail};
use sqlx::PgPool;
use sqlx::migrate::{Migrate, Migrator};

/// What a migration run would do, or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The service's schema.
    pub schema: String,
    /// Versions to apply, oldest first, with their descriptions.
    pub pending: Vec<(i64, String)>,
    /// The newest version already applied, if any.
    pub current: Option<i64>,
}

impl Plan {
    /// Whether the schema is already where this build expects it.
    #[must_use]
    pub fn is_up_to_date(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Works out what applying `migrator` to `pool` would change.
///
/// # Errors
///
/// Returns an error when the bookkeeping table cannot be read or created.
pub async fn plan(pool: &PgPool, schema: &str, migrator: &Migrator) -> Result<Plan> {
    let mut connection = pool.acquire().await.context("acquiring a connection to plan migrations")?;
    connection
        .ensure_migrations_table(&migrator.table_name)
        .await
        .context("preparing the migrations table")?;

    let applied = connection
        .list_applied_migrations(&migrator.table_name)
        .await
        .context("reading applied migrations")?;
    let known: Vec<i64> = applied.iter().map(|m| m.version).collect();

    let pending = migrator
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .filter(|m| !known.contains(&m.version))
        .map(|m| (m.version, m.description.to_string()))
        .collect();

    Ok(Plan {
        schema: schema.to_owned(),
        pending,
        current: known.iter().copied().max(),
    })
}

/// Applies every pending migration.
///
/// # Errors
///
/// Returns an error when a migration fails; sqlx runs each in its own
/// transaction, so a failure leaves the earlier ones applied and this one not.
pub async fn run(pool: &PgPool, migrator: &Migrator) -> Result<()> {
    migrator.run(pool).await.context("applying migrations")?;
    Ok(())
}

/// Rolls the schema back to `target`, undoing every migration newer than it.
///
/// `target` is the version to stop at, not the one to undo: passing the version
/// before the release being withdrawn leaves that release's schema gone. Pass
/// `-1` to undo everything.
///
/// # Errors
///
/// Returns an error when a migration in the range has no `.down.sql`, or when
/// undoing one fails.
pub async fn undo(pool: &PgPool, migrator: &Migrator, target: i64) -> Result<()> {
    // A migration without a down step is only discovered halfway through the
    // rollback, with the newer ones already undone. Refusing up front leaves
    // the schema as it was.
    let irreversible: Vec<i64> = migrator
        .iter()
        .filter(|m| !m.migration_type.is_down_migration() && m.version > target)
        .filter(|up| !migrator.iter().any(|m| m.migration_type.is_down_migration() && m.version == up.version))
        .map(|m| m.version)
        .collect();

    if !irreversible.is_empty() {
        bail!("migrations {irreversible:?} have no .down.sql; rolling back to {target} would be irreversible");
    }

    migrator.undo(pool, target).await.context("undoing migrations")?;
    Ok(())
}
