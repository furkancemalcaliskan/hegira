ALTER TABLE users ADD COLUMN IF NOT EXISTS pending_email TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS email_change_token TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS email_change_sent_at TIMESTAMPTZ;
CREATE UNIQUE INDEX IF NOT EXISTS ux_users_email_change_token
    ON users (email_change_token) WHERE email_change_token IS NOT NULL;
