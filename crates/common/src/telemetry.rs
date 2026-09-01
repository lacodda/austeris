//! Logging setup, identical in every binary.

use tracing_subscriber::EnvFilter;

/// Installs the process-wide log subscriber.
///
/// The level comes from `RUST_LOG`; without it, this crate's own crates talk at
/// `info` and everything else stays quiet.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("austeris=info,tower_http=info,warn"));

    // A second call would panic on a global subscriber that is already set —
    // and tests, which each want logs, are several calls in one process.
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
