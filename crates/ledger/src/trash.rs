//! How long the trash keeps what is put in it, and the hourly erasing of what
//! it has kept long enough.

use std::time::Duration;

use sqlx::PgPool;

use crate::repository;

/// How many days a deleted entry, or a file taken off one, can still come
/// back. Then it is erased for good: the entry with its history and its files.
pub const KEEP_DAYS: i32 = 30;

/// How often the trash is looked through.
const EVERY: Duration = Duration::from_hours(1);

/// Erases, every hour, what has been in the trash longer than [`KEEP_DAYS`].
///
/// Hourly rather than when someone looks: "erased after thirty days" is a
/// promise about what is kept, not about what is shown, and a trash nobody
/// opens for a season must not keep a season of receipts.
pub async fn every_hour(pool: PgPool) {
    let mut ticks = tokio::time::interval(EVERY);
    loop {
        ticks.tick().await;
        match repository::sweep(&pool).await {
            Ok(swept) if swept == repository::Swept::default() => {}
            Ok(swept) => tracing::info!(
                entries = swept.entries,
                attachments = swept.attachments,
                "erased what the trash kept long enough"
            ),
            Err(error) => tracing::error!(error = format!("{error:#}"), "erasing the trash failed"),
        }
    }
}
