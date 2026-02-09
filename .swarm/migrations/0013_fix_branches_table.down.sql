-- Rollback branches table changes

-- Add back status column
ALTER TABLE branches ADD COLUMN status VARCHAR(20) DEFAULT 'active';

-- Add back epic_label column
ALTER TABLE branches ADD COLUMN epic_label VARCHAR(255);

-- Migrate epic_id back to epic_label
UPDATE branches b
SET epic_label = e.epic_label
FROM epics e
WHERE b.epic_id = e.id;

-- Drop epic_id column
DROP INDEX IF EXISTS idx_branches_epic_id;
ALTER TABLE branches DROP COLUMN epic_id;

-- Rename columns back
ALTER TABLE branches RENAME COLUMN branch_name TO name;
ALTER TABLE branches RENAME COLUMN issue_reference TO ticket_ref;

-- Drop merge_commit_hash
ALTER TABLE branches DROP COLUMN merge_commit_hash;

-- Recreate original indexes
CREATE INDEX IF NOT EXISTS idx_branches_epic_label ON branches(epic_label);
CREATE INDEX IF NOT EXISTS idx_branches_status ON branches(status);
