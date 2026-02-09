-- Initial Swarm PostgreSQL Schema
-- This migration creates all the core tables for the Swarm workflow system

-- ============================================================================
-- Projects Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS projects (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    workflow_type VARCHAR(50) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'archived')),
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_projects_status ON projects(status);
CREATE INDEX IF NOT EXISTS idx_projects_workflow_type ON projects(workflow_type);
CREATE INDEX IF NOT EXISTS idx_projects_created_at ON projects(created_at);

-- ============================================================================
-- Project Dependencies Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS project_dependencies (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    depends_on_project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    dependency_type VARCHAR(50) DEFAULT 'blocks',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, depends_on_project_id),
    CHECK(project_id != depends_on_project_id)
);

CREATE INDEX IF NOT EXISTS idx_project_dependencies_project_id ON project_dependencies(project_id);
CREATE INDEX IF NOT EXISTS idx_project_dependencies_depends_on ON project_dependencies(depends_on_project_id);

-- ============================================================================
-- Tickets Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS tickets (
    id SERIAL PRIMARY KEY,
    id_number INTEGER UNIQUE,
    id_string VARCHAR(255) UNIQUE,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    state VARCHAR(20) NOT NULL CHECK (state IN ('open', 'closed')),
    ticket_type VARCHAR(20) NOT NULL DEFAULT 'feature' CHECK (ticket_type IN ('bug', 'feature', 'docs', 'refactor', 'test', 'chore')),
    priority VARCHAR(20) NOT NULL DEFAULT 'medium' CHECK (priority IN ('critical', 'high', 'medium', 'low')),
    epic_label VARCHAR(255),
    milestone VARCHAR(255),
    url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_ticket_id CHECK (
        (id_number IS NOT NULL AND id_string IS NULL) OR
        (id_number IS NULL AND id_string IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_tickets_state ON tickets(state);
CREATE INDEX IF NOT EXISTS idx_tickets_type ON tickets(ticket_type);
CREATE INDEX IF NOT EXISTS idx_tickets_priority ON tickets(priority);
CREATE INDEX IF NOT EXISTS idx_tickets_epic_label ON tickets(epic_label);
CREATE INDEX IF NOT EXISTS idx_tickets_milestone ON tickets(milestone);
CREATE INDEX IF NOT EXISTS idx_tickets_created_at ON tickets(created_at);

-- ============================================================================
-- Pull Requests Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS pull_requests (
    number INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    state VARCHAR(20) NOT NULL CHECK (state IN ('open', 'merged', 'closed')),
    pr_type VARCHAR(20) NOT NULL DEFAULT 'feature' CHECK (pr_type IN ('bug', 'feature', 'docs', 'refactor', 'test', 'chore')),
    priority VARCHAR(20) NOT NULL DEFAULT 'medium' CHECK (priority IN ('critical', 'high', 'medium', 'low')),
    epic_label VARCHAR(255),
    url TEXT NOT NULL,
    branch_name VARCHAR(255) NOT NULL,
    branch_sha VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_prs_state ON pull_requests(state);
CREATE INDEX IF NOT EXISTS idx_prs_type ON pull_requests(pr_type);
CREATE INDEX IF NOT EXISTS idx_prs_priority ON pull_requests(priority);
CREATE INDEX IF NOT EXISTS idx_prs_epic_label ON pull_requests(epic_label);
CREATE INDEX IF NOT EXISTS idx_prs_created_at ON pull_requests(created_at);

-- ============================================================================
-- Users Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    user_string VARCHAR(255) UNIQUE,
    user_number INTEGER UNIQUE,
    user_uuid VARCHAR(255) UNIQUE,
    display_name VARCHAR(255),
    CONSTRAINT check_user_id CHECK (
        user_string IS NOT NULL OR user_number IS NOT NULL OR user_uuid IS NOT NULL
    )
);

-- ============================================================================
-- Ticket Assignees (Many-to-Many)
-- ============================================================================

CREATE TABLE IF NOT EXISTS ticket_assignees (
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (ticket_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_ticket_assignees_user ON ticket_assignees(user_id);

-- ============================================================================
-- PR Assignees (Many-to-Many)
-- ============================================================================

CREATE TABLE IF NOT EXISTS pr_assignees (
    pr_number INTEGER NOT NULL REFERENCES pull_requests(number) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (pr_number, user_id)
);

CREATE INDEX IF NOT EXISTS idx_pr_assignees_user ON pr_assignees(user_id);

-- ============================================================================
-- Comments Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS comments (
    id SERIAL PRIMARY KEY,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE CASCADE,
    pr_number INTEGER REFERENCES pull_requests(number) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_comment_target CHECK (
        (ticket_id IS NOT NULL AND pr_number IS NULL) OR
        (ticket_id IS NULL AND pr_number IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_comments_ticket ON comments(ticket_id);
CREATE INDEX IF NOT EXISTS idx_comments_pr ON comments(pr_number);
CREATE INDEX IF NOT EXISTS idx_comments_created_at ON comments(created_at);

-- ============================================================================
-- Reviews Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS reviews (
    id SERIAL PRIMARY KEY,
    pr_number INTEGER NOT NULL REFERENCES pull_requests(number) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    state VARCHAR(20) NOT NULL CHECK (state IN ('approved', 'changes_requested', 'commented')),
    body TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reviews_pr ON reviews(pr_number);
CREATE INDEX IF NOT EXISTS idx_reviews_state ON reviews(state);
CREATE INDEX IF NOT EXISTS idx_reviews_created_at ON reviews(created_at);

-- ============================================================================
-- Metadata Table (for workflow state tracking)
-- ============================================================================

CREATE TABLE IF NOT EXISTS metadata (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key VARCHAR(255) NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_metadata_project_id ON metadata(project_id);

-- ============================================================================
-- Epic Summaries Table (for iteration summaries)
-- ============================================================================

CREATE TABLE IF NOT EXISTS epic_summaries (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    iteration_number INTEGER NOT NULL,
    epic_label VARCHAR(255) NOT NULL,
    phase VARCHAR(255) NOT NULL,
    brief_summary TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, iteration_number)
);

CREATE INDEX IF NOT EXISTS idx_epic_summaries_project_id ON epic_summaries(project_id);
CREATE INDEX IF NOT EXISTS idx_epic_summaries_epic ON epic_summaries(epic_label);
CREATE INDEX IF NOT EXISTS idx_epic_summaries_phase ON epic_summaries(phase);
CREATE INDEX IF NOT EXISTS idx_epic_summaries_created_at ON epic_summaries(created_at);

-- ============================================================================
-- Epic Descriptions Table (for epic.md content)
-- ============================================================================

CREATE TABLE IF NOT EXISTS epic_descriptions (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    iteration_number INTEGER NOT NULL,
    epic_label VARCHAR(255) NOT NULL,
    phase VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, iteration_number)
);

CREATE INDEX IF NOT EXISTS idx_epic_descriptions_project_id ON epic_descriptions(project_id);
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_epic ON epic_descriptions(epic_label);
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_phase ON epic_descriptions(phase);
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_created_at ON epic_descriptions(created_at);

-- ============================================================================
-- Iteration Reports Table (for report.md content)
-- ============================================================================

CREATE TABLE IF NOT EXISTS iteration_reports (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    iteration_number INTEGER NOT NULL,
    epic_label VARCHAR(255) NOT NULL,
    phase VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, iteration_number)
);

CREATE INDEX IF NOT EXISTS idx_iteration_reports_project_id ON iteration_reports(project_id);
CREATE INDEX IF NOT EXISTS idx_iteration_reports_epic ON iteration_reports(epic_label);
CREATE INDEX IF NOT EXISTS idx_iteration_reports_phase ON iteration_reports(phase);
CREATE INDEX IF NOT EXISTS idx_iteration_reports_created_at ON iteration_reports(created_at);

-- ============================================================================
-- Project Plan Tables (for project-plan.md storage)
-- ============================================================================

CREATE TABLE IF NOT EXISTS phases (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    phase_number INTEGER NOT NULL,
    phase_name VARCHAR(255) NOT NULL,
    goal TEXT,
    duration_estimate VARCHAR(100),
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, phase_number)
);

CREATE INDEX IF NOT EXISTS idx_phases_project_id ON phases(project_id);
CREATE INDEX IF NOT EXISTS idx_phases_phase_number ON phases(phase_number);
CREATE INDEX IF NOT EXISTS idx_phases_position ON phases(position);

CREATE TABLE IF NOT EXISTS epics (
    id SERIAL PRIMARY KEY,
    phase_id INTEGER NOT NULL REFERENCES phases(id) ON DELETE CASCADE,
    epic_number VARCHAR(10) NOT NULL UNIQUE,
    epic_label VARCHAR(255) NOT NULL UNIQUE,
    epic_title VARCHAR(255) NOT NULL,
    description TEXT,
    goal TEXT,
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_epics_phase_id ON epics(phase_id);
CREATE INDEX IF NOT EXISTS idx_epics_epic_label ON epics(epic_label);
CREATE INDEX IF NOT EXISTS idx_epics_epic_number ON epics(epic_number);
CREATE INDEX IF NOT EXISTS idx_epics_position ON epics(position);

CREATE TABLE IF NOT EXISTS epic_capabilities (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    capability TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_epic_capabilities_epic_id ON epic_capabilities(epic_id);

CREATE TABLE IF NOT EXISTS epic_dependencies (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    depends_on_epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    dependency_type VARCHAR(50) DEFAULT 'blocks',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(epic_id, depends_on_epic_id),
    CHECK(epic_id != depends_on_epic_id)
);

CREATE INDEX IF NOT EXISTS idx_epic_dependencies_epic_id ON epic_dependencies(epic_id);
CREATE INDEX IF NOT EXISTS idx_epic_dependencies_depends_on ON epic_dependencies(depends_on_epic_id);

CREATE TABLE IF NOT EXISTS epic_risks (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    risk_description TEXT NOT NULL,
    risk_type VARCHAR(50),
    severity VARCHAR(20),
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_epic_risks_epic_id ON epic_risks(epic_id);
CREATE INDEX IF NOT EXISTS idx_epic_risks_risk_type ON epic_risks(risk_type);
CREATE INDEX IF NOT EXISTS idx_epic_risks_severity ON epic_risks(severity);

CREATE TABLE IF NOT EXISTS project_plan_metadata (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key VARCHAR(255) NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_project_plan_metadata_project_id ON project_plan_metadata(project_id);

-- ============================================================================
-- Commits Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS commits (
    commit_hash VARCHAR(255) PRIMARY KEY,
    branch_name VARCHAR(255),
    epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL,
    commit_type VARCHAR(50),
    scope VARCHAR(255),
    message TEXT NOT NULL,
    body TEXT,
    breaking BOOLEAN DEFAULT FALSE,
    ticket_reference VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_commits_epic_id ON commits(epic_id);
CREATE INDEX IF NOT EXISTS idx_commits_branch_name ON commits(branch_name);
CREATE INDEX IF NOT EXISTS idx_commits_created_at ON commits(created_at);
CREATE INDEX IF NOT EXISTS idx_commits_commit_type ON commits(commit_type);

-- ============================================================================
-- Commit File Changes Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS commit_file_changes (
    id SERIAL PRIMARY KEY,
    commit_hash VARCHAR(255) NOT NULL REFERENCES commits(commit_hash) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    diff TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_commit_file_changes_commit_hash ON commit_file_changes(commit_hash);
CREATE INDEX IF NOT EXISTS idx_commit_file_changes_file_path ON commit_file_changes(file_path);
CREATE INDEX IF NOT EXISTS idx_commit_file_changes_created_at ON commit_file_changes(created_at);

-- ============================================================================
-- Branches Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS branches (
    name VARCHAR(255) PRIMARY KEY,
    base_branch VARCHAR(255),
    epic_label VARCHAR(255),
    ticket_ref VARCHAR(50),
    description TEXT,
    status VARCHAR(20) DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    merged_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_branches_epic_label ON branches(epic_label);
CREATE INDEX IF NOT EXISTS idx_branches_status ON branches(status);

-- ============================================================================
-- Audit Log Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS audit_log (
    id SERIAL PRIMARY KEY,
    entity_type VARCHAR(50) NOT NULL,
    entity_id VARCHAR(255) NOT NULL,
    action VARCHAR(20) NOT NULL CHECK (action IN ('CREATE', 'UPDATE', 'DELETE')),
    user_name VARCHAR(255) NOT NULL,
    old_values JSONB,
    new_values JSONB,
    changed_fields TEXT[],
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_entity ON audit_log(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_name);
CREATE INDEX IF NOT EXISTS idx_audit_log_created_at ON audit_log(created_at);

-- ============================================================================
-- Todos Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS todos (
    id SERIAL PRIMARY KEY,
    commit_sha VARCHAR(255) NOT NULL REFERENCES commits(commit_hash) ON DELETE CASCADE,
    phase_id INTEGER REFERENCES phases(id) ON DELETE SET NULL,
    epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
    todo_comment TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line_number INTEGER,
    description TEXT,
    priority INTEGER NOT NULL DEFAULT 3 CHECK (priority >= 1 AND priority <= 5),
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_implement_before CHECK (
        phase_id IS NOT NULL OR epic_id IS NOT NULL OR ticket_id IS NOT NULL
    )
);

CREATE INDEX IF NOT EXISTS idx_todos_commit_sha ON todos(commit_sha);
CREATE INDEX IF NOT EXISTS idx_todos_phase_id ON todos(phase_id);
CREATE INDEX IF NOT EXISTS idx_todos_epic_id ON todos(epic_id);
CREATE INDEX IF NOT EXISTS idx_todos_ticket_id ON todos(ticket_id);
CREATE INDEX IF NOT EXISTS idx_todos_completed ON todos(completed);
CREATE INDEX IF NOT EXISTS idx_todos_priority ON todos(priority);
CREATE INDEX IF NOT EXISTS idx_todos_file_path ON todos(file_path);
CREATE INDEX IF NOT EXISTS idx_todos_created_at ON todos(created_at);

-- ============================================================================
-- Tasks Table (for agent workflow tracking)
-- ============================================================================

CREATE TABLE IF NOT EXISTS tasks (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    description TEXT NOT NULL,
    instructions TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'in_progress', 'completed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);

-- ============================================================================
-- Helper Functions
-- ============================================================================

CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Triggers for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_projects_updated_at') THEN
        CREATE TRIGGER update_projects_updated_at BEFORE UPDATE ON projects
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_tickets_updated_at') THEN
        CREATE TRIGGER update_tickets_updated_at BEFORE UPDATE ON tickets
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_prs_updated_at') THEN
        CREATE TRIGGER update_prs_updated_at BEFORE UPDATE ON pull_requests
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_metadata_updated_at') THEN
        CREATE TRIGGER update_metadata_updated_at BEFORE UPDATE ON metadata
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_epic_summaries_updated_at') THEN
        CREATE TRIGGER update_epic_summaries_updated_at BEFORE UPDATE ON epic_summaries
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_epic_descriptions_updated_at') THEN
        CREATE TRIGGER update_epic_descriptions_updated_at BEFORE UPDATE ON epic_descriptions
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_iteration_reports_updated_at') THEN
        CREATE TRIGGER update_iteration_reports_updated_at BEFORE UPDATE ON iteration_reports
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_phases_updated_at') THEN
        CREATE TRIGGER update_phases_updated_at BEFORE UPDATE ON phases
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_epics_updated_at') THEN
        CREATE TRIGGER update_epics_updated_at BEFORE UPDATE ON epics
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_project_plan_metadata_updated_at') THEN
        CREATE TRIGGER update_project_plan_metadata_updated_at BEFORE UPDATE ON project_plan_metadata
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_todos_updated_at') THEN
        CREATE TRIGGER update_todos_updated_at BEFORE UPDATE ON todos
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_tasks_updated_at') THEN
        CREATE TRIGGER update_tasks_updated_at BEFORE UPDATE ON tasks
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- ============================================================================
-- Action History Table (for workflow execution tracking)
-- ============================================================================

CREATE TABLE IF NOT EXISTS action_history (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    action_name VARCHAR(255) NOT NULL,
    iteration_number INTEGER,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    duration_seconds INTEGER,
    status VARCHAR(50) NOT NULL CHECK (status IN ('success', 'failed', 'skipped')),
    metadata JSONB
);

CREATE INDEX IF NOT EXISTS idx_action_history_project_id ON action_history(project_id);
CREATE INDEX IF NOT EXISTS idx_action_history_action ON action_history(action_name);
CREATE INDEX IF NOT EXISTS idx_action_history_iteration ON action_history(iteration_number);
CREATE INDEX IF NOT EXISTS idx_action_history_executed_at ON action_history(executed_at DESC);
CREATE INDEX IF NOT EXISTS idx_action_history_action_iteration ON action_history(action_name, iteration_number);

-- ============================================================================
-- Memories Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS memories (
    id SERIAL PRIMARY KEY,
    summary TEXT NOT NULL,
    description TEXT NOT NULL,
    phase_id INTEGER REFERENCES phases(id) ON DELETE SET NULL,
    epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL,
    iteration INTEGER,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
    commit_id VARCHAR(255) REFERENCES commits(commit_hash) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    forgotten_at TIMESTAMPTZ,
    CONSTRAINT check_summary_not_empty CHECK (char_length(trim(summary)) > 0),
    CONSTRAINT check_description_not_empty CHECK (char_length(trim(description)) > 0)
);

CREATE INDEX IF NOT EXISTS idx_memories_active ON memories(forgotten_at) WHERE forgotten_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_phase ON memories(phase_id);
CREATE INDEX IF NOT EXISTS idx_memories_epic ON memories(epic_id);
CREATE INDEX IF NOT EXISTS idx_memories_iteration ON memories(iteration);
CREATE INDEX IF NOT EXISTS idx_memories_ticket ON memories(ticket_id);
CREATE INDEX IF NOT EXISTS idx_memories_commit ON memories(commit_id);
CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at);

-- ============================================================================
-- Initial Data
-- ============================================================================

INSERT INTO users (user_string, display_name) VALUES ('system', 'System')
ON CONFLICT (user_string) DO NOTHING;

-- NOTE: Default project creation was moved to init.rs to support custom workflows
-- The init command now creates the project with the user's specified workflow type
-- For existing installations, the project already exists from a previous migration run
