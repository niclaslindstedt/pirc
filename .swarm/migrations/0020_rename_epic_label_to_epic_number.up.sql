-- Rename state.epic_label to state.epic_number
-- Epic numbers (E001, E002) are the primary identifiers, not labels

-- Rename the column
ALTER TABLE state RENAME COLUMN epic_label TO epic_number;

-- Drop old index
DROP INDEX IF EXISTS idx_state_epic_label;

-- Create new index
CREATE INDEX IF NOT EXISTS idx_state_epic_number ON state(epic_number);
