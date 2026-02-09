-- Background Jobs Table for Parallel Execution
-- This migration adds support for background task queue and parallel action execution

-- ============================================================================
-- Background Jobs Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS background_jobs (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    job_type VARCHAR(50) NOT NULL CHECK (job_type IN ('action', 'hook', 'command', 'batch')),
    action_name VARCHAR(255),
    command TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
    priority INTEGER NOT NULL DEFAULT 50,
    parallel_group VARCHAR(255),
    depends_on_jobs INTEGER[],
    result TEXT,
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_background_jobs_project_id ON background_jobs(project_id);
CREATE INDEX IF NOT EXISTS idx_background_jobs_status ON background_jobs(status);
CREATE INDEX IF NOT EXISTS idx_background_jobs_job_type ON background_jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_background_jobs_priority ON background_jobs(priority DESC);
CREATE INDEX IF NOT EXISTS idx_background_jobs_parallel_group ON background_jobs(parallel_group);
CREATE INDEX IF NOT EXISTS idx_background_jobs_created_at ON background_jobs(created_at);

-- Trigger for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_background_jobs_updated_at') THEN
        CREATE TRIGGER update_background_jobs_updated_at BEFORE UPDATE ON background_jobs
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- ============================================================================
-- Parallel Execution Sessions Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS parallel_sessions (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    session_name VARCHAR(255),
    status VARCHAR(20) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'failed', 'cancelled')),
    max_parallelism INTEGER NOT NULL DEFAULT 4,
    total_jobs INTEGER NOT NULL DEFAULT 0,
    completed_jobs INTEGER NOT NULL DEFAULT 0,
    failed_jobs INTEGER NOT NULL DEFAULT 0,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_parallel_sessions_project_id ON parallel_sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_parallel_sessions_status ON parallel_sessions(status);
CREATE INDEX IF NOT EXISTS idx_parallel_sessions_created_at ON parallel_sessions(created_at);

-- Trigger for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_parallel_sessions_updated_at') THEN
        CREATE TRIGGER update_parallel_sessions_updated_at BEFORE UPDATE ON parallel_sessions
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- Link jobs to sessions
ALTER TABLE background_jobs ADD COLUMN IF NOT EXISTS session_id INTEGER REFERENCES parallel_sessions(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_background_jobs_session_id ON background_jobs(session_id);
