-- Change state.phase from VARCHAR (name) to INTEGER (ID)
-- For referential integrity with phases table

-- Add new column for phase_id
ALTER TABLE state ADD COLUMN phase_id INTEGER REFERENCES phases(id) ON DELETE SET NULL;

-- Migrate existing data: convert phase names to phase IDs
UPDATE state s
SET phase_id = p.id
FROM phases p
WHERE s.phase = p.phase_name;

-- Drop old phase column
ALTER TABLE state DROP COLUMN phase;

-- Rename phase_id to phase
ALTER TABLE state RENAME COLUMN phase_id TO phase;

-- Add index
CREATE INDEX IF NOT EXISTS idx_state_phase ON state(phase);
