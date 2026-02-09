-- Rollback: Convert state.phase from INTEGER (ID) back to VARCHAR (name)

-- Add new column for phase name
ALTER TABLE state ADD COLUMN phase_name VARCHAR(255);

-- Migrate data back: convert phase IDs to phase names
UPDATE state s
SET phase_name = p.phase_name
FROM phases p
WHERE s.phase = p.id;

-- Drop old phase column and index
DROP INDEX IF EXISTS idx_state_phase;
ALTER TABLE state DROP COLUMN phase;

-- Rename phase_name to phase
ALTER TABLE state RENAME COLUMN phase_name TO phase;
