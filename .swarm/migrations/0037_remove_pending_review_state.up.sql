-- Remove 'pending_review' as a stored ticket state
-- Status will now be derived at read time based on whether the ticket has an open CR

-- Convert existing pending_review tickets to open (status will be derived from CR)
UPDATE tickets SET state = 'open' WHERE state = 'pending_review';

-- Drop the existing check constraint
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;

-- Add updated constraint without pending_review
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'closed'));
