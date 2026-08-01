ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS user_id INTEGER;

UPDATE sessions s
SET user_id = u.id
FROM users u
WHERE s.username = u.username
  AND s.user_id IS NULL;

ALTER TABLE sessions
    ALTER COLUMN user_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'fk_sessions_user_id'
    ) THEN
        ALTER TABLE sessions
            ADD CONSTRAINT fk_sessions_user_id
            FOREIGN KEY (user_id)
            REFERENCES users(id)
            ON DELETE CASCADE;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);

ALTER TABLE user_roles
    ADD COLUMN IF NOT EXISTS user_id INTEGER;

UPDATE user_roles ur
SET user_id = u.id
FROM users u
WHERE ur.username = u.username
  AND ur.user_id IS NULL;

ALTER TABLE user_roles
    ALTER COLUMN user_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'fk_user_roles_user_id'
    ) THEN
        ALTER TABLE user_roles
            ADD CONSTRAINT fk_user_roles_user_id
            FOREIGN KEY (user_id)
            REFERENCES users(id)
            ON DELETE CASCADE;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_user_roles_user_id ON user_roles(user_id);

DELETE FROM user_roles a
USING user_roles b
WHERE a.ctid < b.ctid
  AND a.user_id = b.user_id
  AND a.role_name = b.role_name;

CREATE UNIQUE INDEX IF NOT EXISTS ux_user_roles_user_id_role_name
    ON user_roles(user_id, role_name);
