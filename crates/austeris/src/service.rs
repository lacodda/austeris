//! Which services exist, and where the gateway forwards each one's traffic.
//!
//! This is the single place a new service is registered. Adding one means a
//! variant here, a crate, a schema and a compose entry — the checklist from
//! ADR 0001, with this file as its first line.

use std::fmt;

/// A service this binary can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Service {
    /// The only public surface: routes to the others and serves the web UI.
    Gateway,
}

impl Service {
    /// The service's name, as it appears in a command line and in a URL.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
        }
    }

    /// Services the gateway routes to, in the order they are matched.
    ///
    /// The gateway is not among them: it does not forward to itself.
    #[must_use]
    pub fn routed() -> &'static [Self] {
        &[]
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
