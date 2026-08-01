CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pid TEXT NOT NULL UNIQUE,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    reset_token TEXT,
    reset_sent_at TEXT,
    email_verification_token TEXT,
    email_verification_sent_at TEXT,
    email_verified_at TEXT,
    magic_link_token TEXT,
    magic_link_expires_at TEXT,
    deleted_at TEXT,
    totp_secret TEXT,
    totp_enabled_at TEXT,
    totp_backup_code_hashes TEXT NOT NULL DEFAULT '[]',
    totp_login_token TEXT,
    totp_login_expires_at TEXT,
    search_revision INTEGER NOT NULL DEFAULT 0,
    pending_email TEXT,
    email_change_token TEXT,
    email_change_sent_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_users_deleted_at ON users (deleted_at);
CREATE INDEX IF NOT EXISTS idx_users_reset_token ON users (reset_token);
CREATE INDEX IF NOT EXISTS idx_users_email_verification_token ON users (email_verification_token);
CREATE INDEX IF NOT EXISTS idx_users_magic_link_token ON users (magic_link_token);
CREATE UNIQUE INDEX IF NOT EXISTS ux_users_email_change_token
    ON users (email_change_token) WHERE email_change_token IS NOT NULL;

CREATE TABLE IF NOT EXISTS sessions (
    pid TEXT NOT NULL UNIQUE,
    token TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TEXT NOT NULL,
    max_expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions (user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions (expires_at);
CREATE INDEX IF NOT EXISTS idx_sessions_max_expires_at ON sessions (max_expires_at);
