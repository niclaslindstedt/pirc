-- Fix branches table to match code expectations
-- This migration renames columns and adds missing merge_commit_hash

-- Rename 'name' to 'branch_name'
ALTER TABLE branches RENAME COLUMN name TO branch_name;

-- Add epic_id column (nullable, references epics table)
ALTER TABLE branches ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;

-- Migrate existing epic_label data to epic_id (if any epics exist)
UPDATE branches b
SET epic_id = e.id
FROM epics e
WHERE b.epic_label = e.epic_label;

-- Drop the old epic_label column
ALTER TABLE branches DROP COLUMN epic_label;

-- Rename 'ticket_ref' to 'issue_reference'
ALTER TABLE branches RENAME COLUMN ticket_ref TO issue_reference;

-- Add merge_commit_hash column
ALTER TABLE branches ADD COLUMN merge_commit_hash VARCHAR(255);

-- Drop the status column (not used in code, merged_at indicates status)
ALTER TABLE branches DROP COLUMN status;

-- Recreate index with new column name
DROP INDEX IF EXISTS idx_branches_epic_label;
CREATE INDEX IF NOT EXISTS idx_branches_epic_id ON branches(epic_id);

-- Drop status index since we dropped the column
DROP INDEX IF EXISTS idx_branches_status;
