-- Add 'merged' to the allowed ticket states
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'closed', 'merged'));

-- Convert tickets that were closed via CR merge to "merged" state
UPDATE tickets SET state = 'merged', close_reason = NULL
WHERE state = 'closed' AND close_reason LIKE 'Closed via CR #%';
