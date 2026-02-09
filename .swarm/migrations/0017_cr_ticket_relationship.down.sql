-- Remove ticket_id from change_requests table

DROP INDEX IF EXISTS idx_change_requests_ticket_id;

ALTER TABLE change_requests
    DROP COLUMN IF EXISTS ticket_id;
