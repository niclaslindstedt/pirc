-- Restore the constraint with pending_review
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'pending_review', 'closed'));
