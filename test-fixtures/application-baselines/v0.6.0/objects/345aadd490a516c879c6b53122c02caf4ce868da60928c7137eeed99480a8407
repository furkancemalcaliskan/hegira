UPDATE outbox_messages
SET processed_at = NOW(),
    locked_at = NULL,
    lock_owner = NULL,
    last_error = 'retired with the Catalog capability'
WHERE processed_at IS NULL
  AND (
      name LIKE 'catalog.%'
      OR (
          name = 'search.index.v1'
          AND LOWER(payload ->> 'index') LIKE 'catalog%'
      )
  );

DELETE FROM search_projection_versions
WHERE LOWER(index_name) LIKE 'catalog%';

DELETE FROM role_permissions
WHERE permission_name LIKE 'Catalog.%';

DELETE FROM permissions
WHERE name LIKE 'Catalog.%';

DROP TABLE catalog_products;
