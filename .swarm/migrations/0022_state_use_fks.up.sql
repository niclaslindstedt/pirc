-- Migration: Convert state columns to use INTEGER FKs instead of string identifiers
-- Changes epic_number -> current_epic (FK to epics.id)
-- Changes current_ticket -> INTEGER FK to tickets.id
-- Changes last_reviewed_ticket -> INTEGER FK to tickets.id

-- ============================================================================
-- Epic: epic_number (VARCHAR) -> current_epic (INTEGER FK)
-- ============================================================================

-- Add new column
ALTER TABLE state ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;

-- Migrate data: look up epic.id from epic.epic_number
UPDATE state s
SET epic_id = e.id
FROM epics e
WHERE s.epic_number = e.epic_number;

-- Drop old column and index
DROP INDEX IF EXISTS idx_state_epic_number;
ALTER TABLE state DROP COLUMN epic_number;

-- Rename to current_epic
ALTER TABLE state RENAME COLUMN epic_id TO current_epic;

-- Add index
CREATE INDEX IF NOT EXISTS idx_state_current_epic ON state(current_epic);

-- ============================================================================
-- Current Ticket: current_ticket (VARCHAR) -> INTEGER FK
-- ============================================================================

-- Add new column
ALTER TABLE state ADD COLUMN current_ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL;

-- Migrate data: look up ticket.id from ticket.id_string
UPDATE state s
SET current_ticket_id = t.id
FROM tickets t
WHERE s.current_ticket = t.id_string;

-- Drop old column and index
DROP INDEX IF EXISTS idx_state_current_ticket;
ALTER TABLE state DROP COLUMN current_ticket;

-- Rename to current_ticket
ALTER TABLE state RENAME COLUMN current_ticket_id TO current_ticket;

-- Add index
CREATE INDEX IF NOT EXISTS idx_state_current_ticket ON state(current_ticket);

-- ============================================================================
-- Last Reviewed Ticket: last_reviewed_ticket (VARCHAR) -> INTEGER FK
-- ============================================================================

-- Add new column
ALTER TABLE state ADD COLUMN last_reviewed_ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL;

-- Migrate data: look up ticket.id from ticket.id_string
UPDATE state s
SET last_reviewed_ticket_id = t.id
FROM tickets t
WHERE s.last_reviewed_ticket = t.id_string;

-- Drop old column
ALTER TABLE state DROP COLUMN last_reviewed_ticket;

-- Rename to last_reviewed_ticket
ALTER TABLE state RENAME COLUMN last_reviewed_ticket_id TO last_reviewed_ticket;

-- No index needed (less frequently queried)
