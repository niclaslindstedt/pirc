-- Add status and close_reason columns to phases table
-- Status tracks whether a phase is open or closed
-- Close reason captures why the phase was closed

ALTER TABLE phases
ADD COLUMN status VARCHAR(20) NOT NULL DEFAULT 'open',
ADD COLUMN close_reason TEXT;

-- Add index on status for efficient filtering
CREATE INDEX IF NOT EXISTS idx_phases_status ON phases(status);

-- Add constraint to ensure status is either 'open' or 'closed'
-- (phases don't need 'in-progress' since epics handle that granularity)
ALTER TABLE phases
ADD CONSTRAINT phases_status_check CHECK (status IN ('open', 'closed'));
