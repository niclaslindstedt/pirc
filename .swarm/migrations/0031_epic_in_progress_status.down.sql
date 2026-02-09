-- Reverse: Remove 'in-progress' from epic status CHECK constraint
-- Note: This will fail if any epics have 'in-progress' status

-- First, update any in-progress epics to 'open'
UPDATE epics SET status = 'open' WHERE status = 'in-progress';

-- Drop existing constraint
ALTER TABLE epics
DROP CONSTRAINT IF EXISTS epics_status_check;

-- Restore original constraint without 'in-progress'
ALTER TABLE epics
ADD CONSTRAINT epics_status_check CHECK (status IN ('open', 'closed'));
