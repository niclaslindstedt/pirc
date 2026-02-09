-- Revert epic_id columns back to epic_label VARCHAR

-- Decisions table: epic_id -> epic_label
ALTER TABLE decisions ADD COLUMN epic_label VARCHAR;
UPDATE decisions d SET epic_label = e.epic_label FROM epics e WHERE d.epic_id = e.id;
ALTER TABLE decisions DROP COLUMN epic_id;

-- Iteration reports table: epic_id -> epic_label
ALTER TABLE iteration_reports ADD COLUMN epic_label VARCHAR;
UPDATE iteration_reports ir SET epic_label = e.epic_label FROM epics e WHERE ir.epic_id = e.id;
ALTER TABLE iteration_reports DROP COLUMN epic_id;

-- Epic descriptions table: epic_id -> epic_label
ALTER TABLE epic_descriptions ADD COLUMN epic_label VARCHAR;
UPDATE epic_descriptions ed SET epic_label = e.epic_label FROM epics e WHERE ed.epic_id = e.id;
ALTER TABLE epic_descriptions DROP COLUMN epic_id;

-- Tickets table: epic_id -> epic_label
DROP INDEX IF EXISTS idx_tickets_epic_id;
ALTER TABLE tickets ADD COLUMN epic_label VARCHAR;
UPDATE tickets t SET epic_label = e.epic_label FROM epics e WHERE t.epic_id = e.id;
ALTER TABLE tickets DROP COLUMN epic_id;
