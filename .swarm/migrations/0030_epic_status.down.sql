-- Reverse: Remove status and close_reason columns from epics

DROP INDEX IF EXISTS idx_epics_status;

ALTER TABLE epics
DROP CONSTRAINT IF EXISTS epics_status_check;

ALTER TABLE epics
DROP COLUMN IF EXISTS status,
DROP COLUMN IF EXISTS close_reason;
