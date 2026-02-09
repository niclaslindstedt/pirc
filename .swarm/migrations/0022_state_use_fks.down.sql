-- Rollback: Convert state columns back to VARCHAR identifiers

-- ============================================================================
-- Epic: current_epic (INTEGER FK) -> epic_number (VARCHAR)
-- ============================================================================

-- Add old column
ALTER TABLE state ADD COLUMN epic_number VARCHAR(255);

-- Migrate data back
UPDATE state s
SET epic_number = e.epic_number
FROM epics e
WHERE s.current_epic = e.id;

-- Drop new column and index
DROP INDEX IF EXISTS idx_state_current_epic;
ALTER TABLE state DROP COLUMN current_epic;

-- Add old index
CREATE INDEX IF NOT EXISTS idx_state_epic_number ON state(epic_number);

-- ============================================================================
-- Current Ticket: current_ticket (INTEGER FK) -> VARCHAR
-- ============================================================================

-- Add old column
ALTER TABLE state ADD COLUMN current_ticket_tmp VARCHAR(255);

-- Migrate data back
UPDATE state s
SET current_ticket_tmp = t.id_string
FROM tickets t
WHERE s.current_ticket = t.id;

-- Drop new column and index
DROP INDEX IF EXISTS idx_state_current_ticket;
ALTER TABLE state DROP COLUMN current_ticket;

-- Rename to current_ticket
ALTER TABLE state RENAME COLUMN current_ticket_tmp TO current_ticket;

-- Add old index
CREATE INDEX IF NOT EXISTS idx_state_current_ticket ON state(current_ticket);

-- ============================================================================
-- Last Reviewed Ticket: last_reviewed_ticket (INTEGER FK) -> VARCHAR
-- ============================================================================

-- Add old column
ALTER TABLE state ADD COLUMN last_reviewed_ticket_tmp VARCHAR(255);

-- Migrate data back
UPDATE state s
SET last_reviewed_ticket_tmp = t.id_string
FROM tickets t
WHERE s.last_reviewed_ticket = t.id;

-- Drop new column
ALTER TABLE state DROP COLUMN last_reviewed_ticket;

-- Rename to last_reviewed_ticket
ALTER TABLE state RENAME COLUMN last_reviewed_ticket_tmp TO last_reviewed_ticket;
