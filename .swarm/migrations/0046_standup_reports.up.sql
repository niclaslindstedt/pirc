CREATE TABLE standup_reports (
    id SERIAL PRIMARY KEY,
    workspace_id UUID DEFAULT current_setting('app.workspace_id')::uuid,
    project_id INTEGER,
    topic TEXT NOT NULL,
    schema_type TEXT NOT NULL,
    response JSONB NOT NULL,
    model TEXT,
    duration_seconds INTEGER,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE standup_reports ENABLE ROW LEVEL SECURITY;
CREATE POLICY standup_reports_workspace_policy ON standup_reports
    USING (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE standup_reports FORCE ROW LEVEL SECURITY;
