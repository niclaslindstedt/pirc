-- Add 'pending_review' state to tickets table

-- Drop the existing check constraint
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;

-- Add the updated check constraint with pending_review
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'pending_review', 'closed'));
