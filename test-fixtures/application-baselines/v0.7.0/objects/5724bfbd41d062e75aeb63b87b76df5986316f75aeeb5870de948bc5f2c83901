CREATE TABLE search_projection_versions (
    index_name TEXT NOT NULL,
    document_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (index_name, document_id)
);
