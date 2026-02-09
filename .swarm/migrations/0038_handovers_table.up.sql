-- Create handovers table for storing action handover messages
CREATE TABLE IF NOT EXISTS handovers (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL,
    action_name TEXT NOT NULL,
    message TEXT,
    output_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX idx_handovers_project_id ON handovers(project_id);
CREATE INDEX idx_handovers_created_at ON handovers(created_at DESC);
CREATE INDEX idx_handovers_action_name ON handovers(action_name);
