-- Migration 0049: Link commits to Claude sessions
--
-- Adds claude_session_id to commits for tracing which agent session produced each commit.
-- Creates claude_sessions table to store session metadata and log file paths.

-- Link commits to their originating Claude session
ALTER TABLE commits ADD COLUMN IF NOT EXISTS claude_session_id VARCHAR(255);
CREATE INDEX IF NOT EXISTS idx_commits_claude_session ON commits(claude_session_id);

-- Store Claude session metadata (one record per action execution)
CREATE TABLE IF NOT EXISTS claude_sessions (
    id SERIAL PRIMARY KEY,
    workspace_id UUID DEFAULT current_setting('app.workspace_id')::uuid,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    session_id VARCHAR(255) NOT NULL UNIQUE,
    action_name TEXT,
    log_file_path TEXT,
    started_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_claude_sessions_session_id ON claude_sessions(session_id);
CREATE INDEX IF NOT EXISTS idx_claude_sessions_workspace ON claude_sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_claude_sessions_project ON claude_sessions(project_id);

-- Enable RLS for workspace isolation
ALTER TABLE claude_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE claude_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY claude_sessions_workspace_isolation ON claude_sessions
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
