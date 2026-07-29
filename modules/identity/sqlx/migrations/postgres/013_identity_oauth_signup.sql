CREATE TABLE oauth_pending_signups (
    token TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_oauth_pending_signups_identity UNIQUE (provider, provider_user_id)
);

CREATE INDEX idx_oauth_pending_signups_expires_at
    ON oauth_pending_signups (expires_at);

ALTER TABLE users
    ADD COLUMN external_only BOOLEAN NOT NULL DEFAULT FALSE;
