-- The fixture owner selects a permitting/rejecting constraint before execution.
-- Default SQLx per-migration transactions must roll this DDL back on rejection.
CREATE TABLE operation_failed (id INTEGER PRIMARY KEY);
INSERT INTO operation_control (id) VALUES (1);
