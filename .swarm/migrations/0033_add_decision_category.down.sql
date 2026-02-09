-- Remove category column from decisions table
DROP INDEX IF EXISTS idx_decisions_category;
ALTER TABLE decisions DROP COLUMN category;
