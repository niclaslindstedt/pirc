-- Revert merged tickets back to closed state
UPDATE tickets SET state = 'closed', close_reason = 'Reverted from merged state'
WHERE state = 'merged';

-- Restore the original check constraint without 'merged'
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_state_check;
ALTER TABLE tickets ADD CONSTRAINT tickets_state_check
    CHECK (state IN ('open', 'closed'));
