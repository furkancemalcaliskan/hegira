CREATE TABLE IF NOT EXISTS roles (
    name TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS permissions (
    name TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS user_roles (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_name TEXT NOT NULL REFERENCES roles(name) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, role_name)
);

CREATE TABLE IF NOT EXISTS role_permissions (
    role_name TEXT NOT NULL REFERENCES roles(name) ON DELETE CASCADE,
    permission_name TEXT NOT NULL REFERENCES permissions(name) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (role_name, permission_name)
);

CREATE INDEX IF NOT EXISTS idx_roles_deleted_at ON roles (deleted_at);
CREATE INDEX IF NOT EXISTS idx_user_roles_user_id ON user_roles (user_id);

INSERT INTO roles (name) VALUES ('admin') ON CONFLICT (name) DO NOTHING;

INSERT INTO permissions (name) VALUES
    ('Identity.Users'),
    ('Identity.Users.Create'),
    ('Identity.Users.Update'),
    ('Identity.Users.Delete'),
    ('Identity.Authorization')
ON CONFLICT (name) DO NOTHING;

INSERT INTO role_permissions (role_name, permission_name) VALUES
    ('admin', 'Identity.Users'),
    ('admin', 'Identity.Users.Create'),
    ('admin', 'Identity.Users.Update'),
    ('admin', 'Identity.Users.Delete'),
    ('admin', 'Identity.Authorization')
ON CONFLICT (role_name, permission_name) DO NOTHING;
