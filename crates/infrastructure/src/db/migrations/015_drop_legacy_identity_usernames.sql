ALTER TABLE sessions
    DROP COLUMN IF EXISTS username;

ALTER TABLE user_roles
    DROP COLUMN IF EXISTS username;
