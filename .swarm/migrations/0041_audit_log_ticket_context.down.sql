-- Remove ticket_context column from audit_log
DROP INDEX IF EXISTS idx_audit_log_ticket_context;
ALTER TABLE audit_log DROP COLUMN IF EXISTS ticket_context;
