-- Remove close_reason column from tickets table

ALTER TABLE tickets DROP COLUMN IF EXISTS close_reason;
