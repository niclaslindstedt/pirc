-- Revert 'pending_review' state addition from tickets table

-- First, update any pending_review tickets back to open
UPDATE tickets SET state = 'open' WHERE state = 'pending_review';

-- Drop the check constraint with pending_review
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;

-- Add back the original check constraint with only open and closed
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'closed'));
