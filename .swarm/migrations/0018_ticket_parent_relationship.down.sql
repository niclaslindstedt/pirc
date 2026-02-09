-- Remove parent ticket relationship
DROP INDEX IF EXISTS idx_tickets_parent_ticket_id;
ALTER TABLE tickets DROP COLUMN parent_ticket_id;
