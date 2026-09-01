//! Migrations against a real PostgreSQL.
//!
//! This logic lives in the database - `search_path` scoping, sqlx's bookkeeping
//! table, what a rollback actually leaves behind - and none of it is exercised
//! by a pool that never connects. Without `AUSTERIS_DATABASE_URL` these tests
//! skip themselves, and the CI job that owns them fails if they do: a suite
//! that quietly tests nothing reports the same "ok" as one that passed.

use std::time::Duration;

use austeris_common::{Config, db, migrate};
use sqlx::PgPool;
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("tests/migrations");

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: AUSTERIS_DATABASE_URL is not set";

/// Opens a pool on a schema of this test's own, dropped and recreated first so
/// a rerun starts from empty rather than from the last run's leftovers.
async fn pool(schema: &str) -> Option<PgPool> {
    let Ok(database_url) = std::env::var("AUSTERIS_DATABASE_URL") else {
        eprintln!("{SKIP}");
        return None;
    };

    let config = Config {
        database_url: Some(database_url),
        bind: String::new(),
        max_connections: 2,
        acquire_timeout: Duration::from_secs(10),
    };

    // Each test owns a schema, so they can run in parallel without one's
    // rollback erasing another's tables.
    let pool = db::connect(&config, schema).await.expect("connecting to the test database");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA \"{schema}\" CASCADE")))
        .execute(&pool)
        .await
        .expect("dropping the test schema");
    pool.close().await;

    Some(db::connect(&config, schema).await.expect("recreating the test schema"))
}

/// Whether a table exists in the schema this pool is scoped to.
async fn table_exists(pool: &PgPool, table: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass(current_schema() || '.' || $1) IS NOT NULL")
        .bind(table)
        .fetch_one(pool)
        .await
        .expect("asking whether the table exists")
}

#[tokio::test]
async fn a_plan_on_an_empty_schema_lists_every_migration() {
    let Some(pool) = pool("test_plan_empty").await else { return };

    let plan = migrate::plan(&pool, "test_plan_empty", &MIGRATOR).await.expect("planning");

    assert_eq!(plan.current, None, "an empty schema is at no version");
    assert!(!plan.is_up_to_date());
    assert_eq!(
        plan.pending.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        vec![20_260_901_000_001, 20_260_901_000_002],
        "both migrations are pending, oldest first"
    );
    // The plan must not have applied anything: --dry-run means dry.
    assert!(!table_exists(&pool, "thing").await, "planning created a table");
}

#[tokio::test]
async fn a_plan_after_migrating_is_empty() {
    let Some(pool) = pool("test_plan_applied").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");
    let plan = migrate::plan(&pool, "test_plan_applied", &MIGRATOR).await.expect("planning");

    assert!(plan.is_up_to_date(), "nothing is pending after a full run: {:?}", plan.pending);
    assert_eq!(plan.current, Some(20_260_901_000_002));
    assert!(table_exists(&pool, "thing").await);
}

#[tokio::test]
async fn migrations_land_in_the_service_schema_only() {
    let Some(pool) = pool("test_scoping").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");

    // The point of `search_path` scoping: the 2025 code used JDBC's
    // `?currentSchema=`, which PostgreSQL ignores, and every service wrote
    // into `public` while believing otherwise.
    let in_public: bool = sqlx::query_scalar("SELECT to_regclass('public.thing') IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("looking in public");
    assert!(!in_public, "the migration leaked into the public schema");

    let bookkeeping_in_public: bool = sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
        .fetch_one(&pool)
        .await
        .expect("looking for bookkeeping in public");
    assert!(!bookkeeping_in_public, "sqlx recorded the run in the public schema");
}

#[tokio::test]
async fn undoing_to_a_version_leaves_that_version_applied() {
    let Some(pool) = pool("test_undo").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");
    migrate::undo(&pool, &MIGRATOR, 20_260_901_000_001).await.expect("undoing");

    let plan = migrate::plan(&pool, "test_undo", &MIGRATOR).await.expect("planning");
    assert_eq!(plan.current, Some(20_260_901_000_001), "the target version stays applied");
    assert_eq!(plan.pending.len(), 1, "the undone migration is pending again");

    // The second migration added a column; undoing it must have taken it away,
    // while the table the first one created stays.
    assert!(table_exists(&pool, "thing").await, "the rollback went one migration too far");
    let has_note: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'thing' AND column_name = 'note')",
    )
    .fetch_one(&pool)
    .await
    .expect("looking for the column");
    assert!(!has_note, "the column survived its migration being undone");
}

#[tokio::test]
async fn undoing_everything_empties_the_schema() {
    let Some(pool) = pool("test_undo_all").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");
    migrate::undo(&pool, &MIGRATOR, -1).await.expect("undoing everything");

    assert!(!table_exists(&pool, "thing").await, "the table survived a full rollback");
    let plan = migrate::plan(&pool, "test_undo_all", &MIGRATOR).await.expect("planning");
    assert_eq!(plan.current, None);
    assert_eq!(plan.pending.len(), 2);
}

#[tokio::test]
async fn migrating_twice_changes_nothing() {
    let Some(pool) = pool("test_idempotent").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");
    migrate::run(&pool, &MIGRATOR).await.expect("migrating again");

    let plan = migrate::plan(&pool, "test_idempotent", &MIGRATOR).await.expect("planning");
    assert_eq!(plan.current, Some(20_260_901_000_002));
    assert!(plan.is_up_to_date());
}

#[tokio::test]
async fn an_irreversible_rollback_is_refused_before_it_starts() {
    let Some(pool) = pool("test_irreversible").await else { return };

    migrate::run(&pool, &MIGRATOR).await.expect("migrating");

    // A migrator whose newest migration has no down step. Undoing past it can
    // only fail - the question is whether it fails before or after tearing
    // down the migrations that came earlier.
    let one_way = Migrator {
        migrations: MIGRATOR
            .iter()
            .filter(|m| !(m.migration_type.is_down_migration() && m.version == 20_260_901_000_002))
            .cloned()
            .collect(),
        ..Migrator::DEFAULT
    };

    let error = migrate::undo(&pool, &one_way, -1).await.expect_err("undoing an irreversible migration");
    assert!(error.to_string().contains("no .down.sql"), "unexpected error: {error}");

    // Nothing was touched: refusing up front is the whole point.
    assert!(table_exists(&pool, "thing").await, "the schema was torn down before the refusal");
    let plan = migrate::plan(&pool, "test_irreversible", &MIGRATOR).await.expect("planning");
    assert_eq!(plan.current, Some(20_260_901_000_002), "a migration was undone before the refusal");
}
