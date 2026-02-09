-- Add ticket_context column to audit_log for tracking which ticket was active during each audit entry
ALTER TABLE audit_log ADD COLUMN ticket_context VARCHAR(20);

-- Index for efficient filtering by ticket context
CREATE INDEX idx_audit_log_ticket_context ON audit_log(ticket_context);
