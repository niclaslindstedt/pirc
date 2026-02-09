-- Rollback: Rename state.epic_number back to state.epic_label

-- Rename the column back
ALTER TABLE state RENAME COLUMN epic_number TO epic_label;

-- Drop new index
DROP INDEX IF EXISTS idx_state_epic_number;

-- Recreate old index
CREATE INDEX IF NOT EXISTS idx_state_epic_label ON state(epic_label);
