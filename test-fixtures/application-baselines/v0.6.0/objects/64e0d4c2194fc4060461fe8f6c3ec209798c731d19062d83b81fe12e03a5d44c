CREATE TABLE catalog_products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    sku TEXT NOT NULL COLLATE NOCASE,
    price_minor INTEGER NOT NULL CHECK (price_minor >= 0),
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);

CREATE UNIQUE INDEX catalog_products_active_sku_uq
    ON catalog_products (sku COLLATE NOCASE)
    WHERE deleted_at IS NULL;

CREATE INDEX catalog_products_active_created_idx
    ON catalog_products (created_at DESC)
    WHERE deleted_at IS NULL;
