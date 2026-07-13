ALTER TABLE users
    ADD COLUMN IF NOT EXISTS totp_secret TEXT,
    ADD COLUMN IF NOT EXISTS totp_enabled_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS totp_backup_code_hashes TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS totp_login_token TEXT,
    ADD COLUMN IF NOT EXISTS totp_login_expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_users_totp_login_token ON users(totp_login_token);
