CREATE TABLE catalog_products (
    id BIGSERIAL PRIMARY KEY,
    pid UUID NOT NULL UNIQUE,
    name TEXT NOT NULL,
    sku TEXT NOT NULL,
    price_minor BIGINT NOT NULL CHECK (price_minor >= 0),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX catalog_products_active_sku_uq
    ON catalog_products (LOWER(sku))
    WHERE deleted_at IS NULL;

CREATE INDEX catalog_products_active_created_idx
    ON catalog_products (created_at DESC)
    WHERE deleted_at IS NULL;
