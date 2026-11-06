UPDATE outbox_messages
SET processed_at = CURRENT_TIMESTAMP,
    locked_at = NULL,
    lock_owner = NULL,
    last_error = 'retired with the Catalog capability'
WHERE processed_at IS NULL
  AND (
      name LIKE 'catalog.%'
      OR (
          name = 'search.index.v1'
          AND CASE
              WHEN json_valid(payload)
              THEN LOWER(json_extract(payload, '$.index')) LIKE 'catalog%'
              ELSE 0
          END
      )
  );

DELETE FROM search_projection_versions
WHERE LOWER(index_name) LIKE 'catalog%';

DELETE FROM role_permissions
WHERE permission_name LIKE 'Catalog.%';

DELETE FROM permissions
WHERE name LIKE 'Catalog.%';

DROP TABLE catalog_products;
