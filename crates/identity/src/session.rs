//! Sessions, and what slows down someone guessing at a password.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

/// How long a session lives without being used.
///
/// A fortnight: long enough that nobody signs in twice a day, short enough that
/// a forgotten laptop stops being a way in.
pub const LIFETIME_DAYS: i64 = 14;

/// The cookie the browser carries, named for the product so it is obvious in a
/// developer console which server put it there.
pub const COOKIE: &str = "austeris_session";

/// How many refusals within [`LOCKOUT_WINDOW_MINUTES`] lock an address out.
pub const LOCKOUT_THRESHOLD: i64 = 5;

/// How far back failures are counted, and so how long a lockout lasts.
pub const LOCKOUT_WINDOW_MINUTES: i64 = 15;

/// Hex-encoded SHA-256 of a token, which is what `sessions.token_hash` holds.
///
/// Not Argon2: this is a 256-bit string the server generated, so there is no
/// dictionary to slow an attacker down with, and the check runs on every
/// request.
#[must_use]
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    // Hex rather than base64: it survives copying through logs, shells and
    // psql without an encoding argument on either side.
    digest.iter().fold(String::with_capacity(64), |mut acc, byte| {
        use std::fmt::Write;
        let _ = write!(acc, "{byte:02x}");
        acc
    })
}

/// A new session: what the browser gets, and what the database keeps.
#[derive(Debug)]
pub struct Issued {
    /// Handed to the client once, in a cookie, and stored nowhere.
    pub token: String,
    /// When it stops working if nobody uses it.
    pub expires_at: DateTime<Utc>,
}

/// Creates a session for a user.
///
/// # Errors
///
/// Returns an error when the session cannot be stored.
pub async fn issue(pool: &PgPool, user_id: Uuid) -> Result<Issued> {
    use rand::RngExt;

    // 32 bytes from the OS: the token is the entire credential, so it has to be
    // unguessable rather than merely unique.
    let bytes: [u8; 32] = rand::rng().random();
    let token = bytes.iter().fold(String::with_capacity(64), |mut acc, byte| {
        use std::fmt::Write;
        let _ = write!(acc, "{byte:02x}");
        acc
    });
    let expires_at = Utc::now() + Duration::days(LIFETIME_DAYS);

    sqlx::query("INSERT INTO sessions (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(hash_token(&token))
        .bind(expires_at)
        .execute(pool)
        .await
        .context("storing the session")?;

    Ok(Issued { token, expires_at })
}

/// Who a session belongs to, if it is still good for anything.
#[derive(Debug, Clone)]
pub struct Holder {
    /// The session itself, so signing out can end this one only.
    pub session_id: Uuid,
    /// The person - the only thing other services are ever told.
    pub user_id: Uuid,
    /// Read alongside the rest so `/me` needs no second query.
    pub email: String,
    /// Likewise.
    pub display_name: String,
}

/// Resolves a token to its holder, refusing expired sessions and inactive people.
///
/// # Errors
///
/// Returns an error when the lookup fails; an unknown or expired token is
/// `Ok(None)`, not an error.
pub async fn authenticate(pool: &PgPool, token: &str) -> Result<Option<Holder>> {
    // The expiry is checked in the query rather than in Rust: a row that
    // outlived its welcome must not authenticate anyone even if the sweep that
    // deletes it has not run.
    let row: Option<(Uuid, Uuid, String, String)> = sqlx::query_as(
        "SELECT s.id, s.user_id, u.email, u.display_name FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > now() AND u.active",
    )
    .bind(hash_token(token))
    .fetch_optional(pool)
    .await
    .context("looking up the session")?;

    let Some((session_id, user_id, email, display_name)) = row else {
        return Ok(None);
    };

    // Rolling expiry, best-effort: someone working through the day should not
    // be signed out mid-afternoon, and failing to extend costs them nothing
    // worse than signing in again.
    if let Err(error) = sqlx::query("UPDATE sessions SET last_used_at = now(), expires_at = now() + ($2 || ' days')::interval WHERE id = $1")
        .bind(session_id)
        .bind(LIFETIME_DAYS.to_string())
        .execute(pool)
        .await
    {
        tracing::warn!(%error, %session_id, "failed to extend the session");
    }

    Ok(Some(Holder {
        session_id,
        user_id,
        email,
        display_name,
    }))
}

/// Ends one session - what signing out does.
///
/// # Errors
///
/// Returns an error when the deletion fails.
pub async fn revoke(pool: &PgPool, session_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1").bind(session_id).execute(pool).await?;
    Ok(())
}

/// Ends every session a user has - the answer to a laptop left on a train.
///
/// # Errors
///
/// Returns an error when the deletion fails.
pub async fn revoke_all(pool: &PgPool, user_id: Uuid) -> Result<u64> {
    let ended = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(ended)
}

/// Deletes sessions that have expired.
///
/// Not needed for correctness - [`authenticate`] already refuses them - but a
/// table that only grows is one nobody wants to meet in a year.
///
/// # Errors
///
/// Returns an error when the sweep fails.
pub async fn sweep_expired(pool: &PgPool) -> Result<u64> {
    let deleted = sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
        .execute(pool)
        .await?
        .rows_affected();
    Ok(deleted)
}

/// Records a refused sign-in.
///
/// The attempted address is kept because a run of failures against one address
/// is the thing worth seeing; the password never is, not even its length.
pub async fn record_failure(pool: &PgPool, attempted_email: &str) {
    if let Err(error) = sqlx::query("INSERT INTO login_failures (attempted_email) VALUES ($1)")
        .bind(attempted_email)
        .execute(pool)
        .await
    {
        // Losing the record must not turn a refused sign-in into an error the
        // caller could tell apart from a wrong password.
        tracing::warn!(%error, "failed to record a refused sign-in");
    }
}

/// Whether an address is currently locked out.
///
/// Keyed by the attempted address rather than by an account, so guessing at an
/// address that does not exist is slowed down the same way - otherwise the
/// lockout itself becomes a way to learn which addresses are real.
///
/// # Errors
///
/// Returns an error when the count cannot be read.
pub async fn is_locked_out(pool: &PgPool, attempted_email: &str) -> Result<bool> {
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM login_failures
         WHERE lower(attempted_email) = lower($1) AND attempted_at > now() - ($2 || ' minutes')::interval",
    )
    .bind(attempted_email)
    .bind(LOCKOUT_WINDOW_MINUTES.to_string())
    .fetch_one(pool)
    .await
    .context("counting recent sign-in failures")?;

    Ok(failures >= LOCKOUT_THRESHOLD)
}

/// Forgets an address's failures, which a successful sign-in does.
///
/// # Errors
///
/// Returns an error when the deletion fails.
pub async fn clear_failures(pool: &PgPool, attempted_email: &str) -> Result<()> {
    sqlx::query("DELETE FROM login_failures WHERE lower(attempted_email) = lower($1)")
        .bind(attempted_email)
        .execute(pool)
        .await
        .context("clearing sign-in failures")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hash_token;

    #[test]
    fn a_token_hashes_to_sixty_four_hex_characters() {
        let hash = hash_token("a token");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_hash_does_not_carry_the_token() {
        assert!(!hash_token("secret").contains("secret"));
    }

    #[test]
    fn different_tokens_hash_differently() {
        assert_ne!(hash_token("one"), hash_token("two"));
        assert_eq!(
            hash_token("one"),
            hash_token("one"),
            "hashing is deterministic, or no session could be looked up"
        );
    }
}
