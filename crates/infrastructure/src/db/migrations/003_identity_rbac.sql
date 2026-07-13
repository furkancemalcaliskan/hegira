CREATE TABLE IF NOT EXISTS roles (
    name TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS permissions (
    name TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_roles (
    username TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    role_name TEXT NOT NULL REFERENCES roles(name) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (username, role_name)
);

CREATE TABLE IF NOT EXISTS role_permissions (
    role_name TEXT NOT NULL REFERENCES roles(name) ON DELETE CASCADE,
    permission_name TEXT NOT NULL REFERENCES permissions(name) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (role_name, permission_name)
);

INSERT INTO roles (name)
VALUES ('admin')
ON CONFLICT (name) DO NOTHING;

INSERT INTO permissions (name)
VALUES
    ('Identity.Users'),
    ('Identity.Users.Create'),
    ('Identity.Users.Update'),
    ('Identity.Users.Delete'),
    ('Identity.Authorization')
ON CONFLICT (name) DO NOTHING;

INSERT INTO role_permissions (role_name, permission_name)
VALUES
    ('admin', 'Identity.Users'),
    ('admin', 'Identity.Users.Create'),
    ('admin', 'Identity.Users.Update'),
    ('admin', 'Identity.Users.Delete'),
    ('admin', 'Identity.Authorization')
ON CONFLICT (role_name, permission_name) DO NOTHING;
