ALTER TABLE sessions ADD COLUMN IF NOT EXISTS pid UUID;
UPDATE sessions SET pid = gen_random_uuid() WHERE pid IS NULL;
ALTER TABLE sessions ALTER COLUMN pid SET DEFAULT gen_random_uuid();
ALTER TABLE sessions ALTER COLUMN pid SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_sessions_pid ON sessions (pid);
