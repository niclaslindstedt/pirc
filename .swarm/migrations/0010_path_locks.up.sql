-- Migration: Path locks for parallel agent work isolation
-- Enables multiple agents to work on the same repository without file conflicts
-- by locking file/directory paths when agents checkout to worktrees

CREATE TABLE IF NOT EXISTS path_locks (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    locked_by VARCHAR(255) NOT NULL,
    worktree_path TEXT NOT NULL,
    branch_name VARCHAR(255) NOT NULL,
    ticket_id VARCHAR(50),
    feature_name VARCHAR(255),
    expires_at TIMESTAMPTZ NOT NULL,
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    released_at TIMESTAMPTZ,
    release_reason VARCHAR(50) CHECK (release_reason IN ('merged', 'expired', 'manual'))
);

-- Only one active lock per path per project (partial unique index)
CREATE UNIQUE INDEX idx_path_locks_active_unique
    ON path_locks (project_id, path) WHERE released_at IS NULL;

-- Index for expired lock cleanup
CREATE INDEX idx_path_locks_expires_at ON path_locks(expires_at) WHERE released_at IS NULL;

-- Index for branch-based operations
CREATE INDEX idx_path_locks_branch ON path_locks(branch_name) WHERE released_at IS NULL;

-- Index for agent-based queries
CREATE INDEX idx_path_locks_locked_by ON path_locks(locked_by) WHERE released_at IS NULL;

-- Index for project-scoped queries
CREATE INDEX idx_path_locks_project_id ON path_locks(project_id);

-- Trigger to update last_activity_at on UPDATE
CREATE OR REPLACE FUNCTION update_path_locks_last_activity()
RETURNS TRIGGER AS $$
BEGIN
    -- Only update last_activity_at if it wasn't explicitly set in the UPDATE
    IF NEW.last_activity_at = OLD.last_activity_at THEN
        NEW.last_activity_at = NOW();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_path_locks_last_activity ON path_locks;
CREATE TRIGGER trigger_path_locks_last_activity
    BEFORE UPDATE ON path_locks
    FOR EACH ROW
    EXECUTE FUNCTION update_path_locks_last_activity();
