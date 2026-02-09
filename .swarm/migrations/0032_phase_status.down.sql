-- Remove status-related columns from phases table

DROP INDEX IF EXISTS idx_phases_status;

ALTER TABLE phases
DROP CONSTRAINT IF EXISTS phases_status_check;

ALTER TABLE phases
DROP COLUMN IF EXISTS close_reason;

ALTER TABLE phases
DROP COLUMN IF EXISTS status;
