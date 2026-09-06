CREATE TABLE search_projection_versions (
    index_name TEXT NOT NULL,
    document_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (index_name, document_id)
);
