-- Drop history table
DROP INDEX IF EXISTS idx_history_entity;
DROP INDEX IF EXISTS idx_history_timestamp;
DROP INDEX IF EXISTS idx_history_project_id;
DROP TABLE IF EXISTS history;
