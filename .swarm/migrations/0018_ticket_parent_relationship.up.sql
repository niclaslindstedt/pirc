-- Add parent ticket relationship to support follow-up tickets
-- Note: parent_ticket_id stores the ticket ID as a string (e.g., "T001")
-- We don't use a foreign key here because the ID format varies (T001, PROJ-123, etc.)
-- and there's no single column to reference
ALTER TABLE tickets ADD COLUMN parent_ticket_id TEXT;

-- Index for efficient parent lookup
CREATE INDEX idx_tickets_parent_ticket_id ON tickets(parent_ticket_id);
