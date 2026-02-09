-- Add status and close_reason columns to epics table
-- Status tracks whether an epic is open or closed
-- Close reason captures why the epic was closed

ALTER TABLE epics
ADD COLUMN status VARCHAR(20) NOT NULL DEFAULT 'open',
ADD COLUMN close_reason TEXT;

-- Add index on status for efficient filtering
CREATE INDEX IF NOT EXISTS idx_epics_status ON epics(status);

-- Add constraint to ensure status is either 'open' or 'closed'
ALTER TABLE epics
ADD CONSTRAINT epics_status_check CHECK (status IN ('open', 'closed'));
