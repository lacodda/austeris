//! Password hashing.
//!
//! A password is human-chosen and therefore guessable at scale, so it gets
//! Argon2id with a per-password salt. Session tokens get SHA-256 instead
//! ([`crate::session`]): they are full-entropy strings this service issued, and
//! there is no dictionary to slow anyone down with.

use anyhow::{Result, anyhow};
use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};

/// Hashes a password for storage.
///
/// # Errors
///
/// Returns an error when the hasher itself fails, which in practice means the
/// system is out of memory.
pub fn hash(password: &str) -> Result<String> {
    // The salt comes from the crate's own generator rather than one built here:
    // a salt is exactly the parameter a caller should not be trusted to supply.
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow!("failed to hash the password: {error}"))
}

/// Checks a password against a stored hash.
///
/// Any failure is `false`, never an error: a malformed hash in the database and
/// a wrong password are the same answer to whoever is asking, and telling them
/// apart is information the caller has no business acting on differently.
#[must_use]
pub fn verify(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        tracing::error!("a stored password hash could not be parsed; the account cannot be signed into");
        return false;
    };

    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// A real Argon2 hash of a value nobody knows.
///
/// Verified against when the address is unknown, so that a refusal costs the
/// same either way and the login form cannot be timed to learn who has an
/// account here.
pub const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHR2YWx1ZQ$K7gNU3sdo+OL0wNhqoVWhr3g6s1xYv72ol/pe/Unols";

#[cfg(test)]
mod tests {
    use super::{DUMMY_HASH, hash, verify};

    #[test]
    fn a_password_verifies_against_its_own_hash_and_nothing_else() {
        let stored = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &stored));
        assert!(!verify("Correct horse battery staple", &stored), "verification is exact");
        assert!(!verify("", &stored));
    }

    #[test]
    fn the_stored_form_reveals_nothing() {
        let stored = hash("hunter2").unwrap();
        assert!(!stored.contains("hunter2"), "the password must not survive in the hash");
        assert!(stored.starts_with("$argon2id$"), "a memory-hard hash, not a bare digest: {stored}");
    }

    #[test]
    fn the_same_password_hashes_differently_every_time() {
        // The salt is what makes two people who chose the same password
        // indistinguishable in a database dump.
        let first = hash("same").unwrap();
        let second = hash("same").unwrap();
        assert_ne!(first, second);
        assert!(verify("same", &first) && verify("same", &second));
    }

    #[test]
    fn a_damaged_hash_refuses_rather_than_admits() {
        // Truncation, a stray edit in psql, a half-written migration: none of
        // it may become a way in.
        assert!(!verify("anything", ""));
        assert!(!verify("anything", "not-a-hash"));
        assert!(!verify("anything", "$argon2id$v=19$m=19456,t=2,p=1$truncated"));
    }

    #[test]
    fn the_dummy_hash_is_a_real_hash_that_nothing_matches() {
        // It exists to make the refusal path cost the same as the success one.
        // A malformed constant would return instantly and reintroduce exactly
        // the timing difference it is there to remove.
        assert!(
            PasswordHashParses(DUMMY_HASH).parses(),
            "the dummy hash must parse, or verification returns early"
        );
        assert!(!verify("", DUMMY_HASH));
        assert!(!verify("password", DUMMY_HASH));
    }

    /// Asks whether a stored hash is well-formed, without exposing the answer
    /// anywhere outside this test.
    struct PasswordHashParses(&'static str);

    impl PasswordHashParses {
        fn parses(&self) -> bool {
            argon2::password_hash::phc::PasswordHash::new(self.0).is_ok()
        }
    }
}
