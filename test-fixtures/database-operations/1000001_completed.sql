-- Disposable database-operation gate only; never a canonical/released migration.
CREATE TABLE operation_completed (id INTEGER PRIMARY KEY);
INSERT INTO operation_completed (id) VALUES (7);
