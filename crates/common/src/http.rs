//! The one way austeris makes an HTTP client.
//!
//! Some calls leave the machine - a price source is an HTTPS endpoint on the
//! internet, and `austeris add` may be pointed at an installation behind a TLS
//! terminator - so every client speaks TLS. rustls needs its cryptography
//! provider chosen before the first client is built, and reqwest panics when
//! none is; choosing it here, once, is what lets every other module ask for a
//! client without knowing that. `clippy.toml` refuses the other ways of making
//! one.

use std::sync::Once;

static PROVIDER: Once = Once::new();

/// A client for plain HTTP and HTTPS alike.
#[must_use]
#[allow(clippy::disallowed_methods, reason = "this is the one place a client is built")]
pub fn client() -> reqwest::Client {
    install_provider();
    reqwest::Client::new()
}

/// Makes `ring` the process's TLS provider, once.
///
/// `ring` rather than reqwest's default, `aws-lc-rs`: sqlx already brings
/// `ring` in for its own TLS, and a second cryptography library would be a C
/// toolchain the arm64 image has to build.
fn install_provider() {
    PROVIDER.call_once(|| {
        // An error means a provider is already installed - by a test harness or
        // another library - which is the state this function exists to reach.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_client_can_be_built_without_anyone_installing_a_provider_first() {
        // reqwest panics right here when no provider is installed.
        let _client = super::client();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
