-- Drop goals table and related triggers/functions
DROP TRIGGER IF EXISTS trigger_goals_updated_at ON goals;
DROP FUNCTION IF EXISTS update_goals_updated_at();
DROP TABLE IF EXISTS goals;
