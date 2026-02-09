-- Remove tags column from decisions table
DROP INDEX IF EXISTS idx_decisions_tags;
ALTER TABLE decisions DROP COLUMN tags;
