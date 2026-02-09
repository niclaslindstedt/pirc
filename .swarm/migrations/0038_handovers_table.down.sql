-- Drop handovers table
DROP INDEX IF EXISTS idx_handovers_action_name;
DROP INDEX IF EXISTS idx_handovers_created_at;
DROP INDEX IF EXISTS idx_handovers_project_id;
DROP TABLE IF EXISTS handovers;
