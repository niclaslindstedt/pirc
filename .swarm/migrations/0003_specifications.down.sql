-- Drop specifications table and related triggers
DROP TRIGGER IF EXISTS trigger_specifications_updated_at ON specifications;
DROP FUNCTION IF EXISTS update_specifications_updated_at();
DROP TABLE IF EXISTS specifications;
