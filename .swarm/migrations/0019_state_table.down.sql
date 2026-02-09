-- Rollback State Table Migration

DROP TRIGGER IF EXISTS update_state_updated_at ON state;
DROP INDEX IF EXISTS idx_state_current_cr;
DROP INDEX IF EXISTS idx_state_current_ticket;
DROP INDEX IF EXISTS idx_state_phase;
DROP INDEX IF EXISTS idx_state_epic_label;
DROP INDEX IF EXISTS idx_state_status;
DROP INDEX IF EXISTS idx_state_current_iteration;
DROP TABLE IF EXISTS state;
