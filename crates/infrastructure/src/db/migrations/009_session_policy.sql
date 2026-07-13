ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS max_expires_at TIMESTAMPTZ;

UPDATE sessions
SET max_expires_at = expires_at
WHERE max_expires_at IS NULL;

ALTER TABLE sessions
    ALTER COLUMN max_expires_at SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions (expires_at);
CREATE INDEX IF NOT EXISTS idx_sessions_max_expires_at ON sessions (max_expires_at);
