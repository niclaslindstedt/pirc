-- Remove priority field and related constraints
DROP INDEX IF EXISTS idx_epics_priority;
ALTER TABLE epics DROP CONSTRAINT IF EXISTS chk_epic_priority;
ALTER TABLE epics DROP COLUMN IF EXISTS priority;
