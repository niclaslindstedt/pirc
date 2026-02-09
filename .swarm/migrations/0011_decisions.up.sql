-- Decision log table for tracking architectural/technical decisions (similar to ADRs)
CREATE TABLE decisions (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    decision_number INTEGER NOT NULL,
    ticket_id_number INTEGER,
    phase_id INTEGER REFERENCES phases(id),
    epic_label VARCHAR(255),
    decision TEXT NOT NULL,
    rationale TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(project_id, decision_number)
);

CREATE INDEX idx_decisions_project ON decisions(project_id);
CREATE INDEX idx_decisions_ticket ON decisions(ticket_id_number);
CREATE INDEX idx_decisions_epic ON decisions(epic_label);
