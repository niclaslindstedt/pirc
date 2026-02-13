CREATE TABLE IF NOT EXISTS workflow_events (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL,
    event_name TEXT NOT NULL,
    body TEXT,
    source_action TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    consumed_at TIMESTAMPTZ,
    consumed_by_action TEXT
);

CREATE INDEX IF NOT EXISTS idx_workflow_events_project_status ON workflow_events (project_id, status);
CREATE INDEX IF NOT EXISTS idx_workflow_events_event_name ON workflow_events (event_name);
CREATE INDEX IF NOT EXISTS idx_workflow_events_created_at ON workflow_events (created_at);
