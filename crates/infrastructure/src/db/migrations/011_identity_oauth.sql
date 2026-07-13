CREATE TABLE oauth_states (
    state TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    csrf_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_oauth_states_expires_at ON oauth_states (expires_at);

CREATE TABLE user_oauth_connections (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (provider, provider_user_id),
    CONSTRAINT uq_user_oauth_connections_user_provider UNIQUE (user_id, provider)
);

CREATE INDEX idx_user_oauth_connections_user_id ON user_oauth_connections (user_id);
