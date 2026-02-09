-- Add 'in-progress' to epic status CHECK constraint
-- This enables automatic state transitions:
-- - open -> in-progress: when first ticket is created for epic
-- - in-progress -> closed: when all tickets are closed

-- Drop existing constraint
ALTER TABLE epics
DROP CONSTRAINT IF EXISTS epics_status_check;

-- Add updated constraint with 'in-progress' state
ALTER TABLE epics
ADD CONSTRAINT epics_status_check CHECK (status IN ('open', 'in-progress', 'closed'));
