-- Add ticket_id to change_requests table for 1-to-1 relationship
--
-- CRs and tickets have a 1-to-1 relationship. The ticket_id should be stored
-- directly on the CR to enable reliable lookups and automatic ticket closure
-- when a CR is approved.

ALTER TABLE change_requests
    ADD COLUMN ticket_id VARCHAR(255);

-- Create index for lookups by ticket_id
CREATE INDEX IF NOT EXISTS idx_change_requests_ticket_id ON change_requests(ticket_id);
