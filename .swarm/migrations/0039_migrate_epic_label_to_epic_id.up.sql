-- Migrate epic_label columns to epic_id (INTEGER FK) in tickets, epic_descriptions, iteration_reports, decisions

-- Tickets table: epic_label VARCHAR -> epic_id INTEGER FK
ALTER TABLE tickets ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;
UPDATE tickets t SET epic_id = e.id FROM epics e WHERE LOWER(t.epic_label) = LOWER(e.epic_label);
ALTER TABLE tickets DROP COLUMN epic_label;
CREATE INDEX idx_tickets_epic_id ON tickets(epic_id);

-- Epic descriptions table: epic_label -> epic_id INTEGER FK
ALTER TABLE epic_descriptions ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;
UPDATE epic_descriptions ed SET epic_id = e.id FROM epics e WHERE LOWER(ed.epic_label) = LOWER(e.epic_label);
ALTER TABLE epic_descriptions DROP COLUMN epic_label;

-- Iteration reports table: epic_label -> epic_id INTEGER FK
ALTER TABLE iteration_reports ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;
UPDATE iteration_reports ir SET epic_id = e.id FROM epics e WHERE LOWER(ir.epic_label) = LOWER(e.epic_label);
ALTER TABLE iteration_reports DROP COLUMN epic_label;

-- Decisions table: epic_label -> epic_id INTEGER FK
ALTER TABLE decisions ADD COLUMN epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL;
UPDATE decisions d SET epic_id = e.id FROM epics e WHERE LOWER(d.epic_label) = LOWER(e.epic_label);
ALTER TABLE decisions DROP COLUMN epic_label;
