ALTER TABLE users ADD COLUMN external_only INTEGER NOT NULL DEFAULT 0 CHECK (external_only IN (0, 1));

CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL,
    csrf_token TEXT NOT NULL,
    flow TEXT NOT NULL DEFAULT 'login' CHECK (flow IN ('login', 'link')),
    username TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    CHECK (flow <> 'link' OR username IS NOT NULL)
);

CREATE INDEX idx_oauth_states_expires_at ON oauth_states (expires_at);

CREATE TABLE user_oauth_connections (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (provider, provider_user_id),
    UNIQUE (user_id, provider)
);

CREATE INDEX idx_user_oauth_connections_user_id ON user_oauth_connections (user_id);

CREATE TABLE oauth_pending_signups (
    token TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    email TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    UNIQUE (provider, provider_user_id)
);

CREATE INDEX idx_oauth_pending_signups_expires_at ON oauth_pending_signups (expires_at);
