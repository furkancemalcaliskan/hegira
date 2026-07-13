CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE SEQUENCE IF NOT EXISTS users_id_seq;

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS id INTEGER,
    ADD COLUMN IF NOT EXISTS pid UUID;

ALTER SEQUENCE users_id_seq OWNED BY users.id;

ALTER TABLE users
    ALTER COLUMN id SET DEFAULT nextval('users_id_seq'),
    ALTER COLUMN pid SET DEFAULT gen_random_uuid();

UPDATE users
SET id = nextval('users_id_seq')
WHERE id IS NULL;

UPDATE users
SET pid = gen_random_uuid()
WHERE pid IS NULL;

SELECT setval(
    'users_id_seq',
    COALESCE((SELECT MAX(id) FROM users), 1),
    EXISTS(SELECT 1 FROM users)
);

ALTER TABLE users
    ALTER COLUMN id SET NOT NULL,
    ALTER COLUMN pid SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS ux_users_id ON users(id);
CREATE UNIQUE INDEX IF NOT EXISTS ux_users_pid ON users(pid);
