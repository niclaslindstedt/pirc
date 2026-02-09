-- Rollback: Remove path locks table and related objects

DROP TRIGGER IF EXISTS trigger_path_locks_last_activity ON path_locks;
DROP FUNCTION IF EXISTS update_path_locks_last_activity();
DROP TABLE IF EXISTS path_locks;
