CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE users (
    id              uuid PRIMARY KEY,
    email           citext UNIQUE NOT NULL,
    display_name    text NOT NULL,
    email_verified  boolean NOT NULL DEFAULT false,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE credentials (
    user_id       uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash text NOT NULL            -- argon2id PHC string
);

CREATE TABLE sessions (
    id                 uuid PRIMARY KEY,
    user_id            uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_hash       bytea NOT NULL,     -- SHA-256 of refresh token
    family_id          uuid NOT NULL,      -- rotation family for reuse detection
    user_agent         text,
    ip                 inet,
    expires_at         timestamptz NOT NULL,
    revoked_at         timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_family ON sessions (family_id);

-- Not given explicit DDL in docs/design.md (only named in the identity module's table list);
-- shaped to match its stated usage (US-01/US-03: hashed, TTL, single-use) and the existing
-- sessions.refresh_hash pattern. TODO: Verify against the source doc if it's ever updated.
CREATE TABLE email_tokens (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  bytea NOT NULL,
    purpose     text NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX email_tokens_user_purpose ON email_tokens (user_id, purpose) WHERE used_at IS NULL;
