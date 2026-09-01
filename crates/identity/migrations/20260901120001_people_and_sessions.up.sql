-- The people who use this installation, and how they stay signed in.
--
-- Everything here lives in the `identity` schema: the pool's `search_path`
-- puts it there, and no other service reads these tables (ADR 0001). A service
-- that needs to know who is calling asks identity over gRPC.

CREATE TABLE users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Login handle. Case-insensitive uniqueness is enforced by the index
    -- below; the spelling the person chose is preserved for display.
    email         text        NOT NULL,
    display_name  text        NOT NULL,
    -- Argon2id PHC string. Null while an account exists with no way in yet.
    password_hash text,
    -- Deactivation rather than deletion: entries and trades reference their
    -- author, and a household's history must survive someone leaving it.
    active        bool        NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX users_email_key ON users (lower(email));

-- Server-side sessions, not signed self-contained tokens. A stateless token
-- cannot be withdrawn, and the blocklist that would fix that is this table
-- under another name - while this installation must be able to end access now,
-- on the afternoon a laptop goes missing.
CREATE TABLE sessions (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- Only the hash: a database dump, or a row in a log line, must not hand
    -- out a working session.
    token_hash   text        NOT NULL UNIQUE,
    expires_at   timestamptz NOT NULL,
    -- Rolling window: a session in daily use should not expire mid-afternoon,
    -- and one nobody has touched for a fortnight should.
    last_used_at timestamptz NOT NULL DEFAULT now(),
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
-- Expired rows are swept on a schedule; the index keeps that sweep cheap.
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

-- Refused sign-ins, kept long enough to lock an account being guessed at.
--
-- Keyed by the address that was attempted rather than by a user: an attacker
-- guessing at an address that does not exist must be slowed down the same way,
-- or the lockout itself becomes a way to learn which addresses are real.
CREATE TABLE login_failures (
    id               bigserial PRIMARY KEY,
    attempted_email  text        NOT NULL,
    attempted_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX login_failures_email_time_idx ON login_failures (lower(attempted_email), attempted_at DESC);
