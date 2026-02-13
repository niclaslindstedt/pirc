-- Migration 0044: Add instance_id UUID column + Row-Level Security (RLS)
--
-- Adds tenant isolation via PostgreSQL RLS. Every table gets:
--   1. instance_id UUID column (backfilled from session variable)
--   2. DEFAULT current_setting('app.instance_id')::uuid (auto-tags new rows)
--   3. RLS policy filtering by instance_id (transparent to queries)
--
-- The app.instance_id session variable MUST be set before this migration runs.
-- For new instances it's a UUID v4; legacy 8-hex-char IDs are converted to
-- deterministic UUID v5 values at application startup.

-- ============================================================================
-- Step 1: Resolve the instance_id for backfilling existing rows
-- ============================================================================

DO $$
DECLARE
    _iid UUID;
BEGIN
    -- The application sets app.instance_id on every connection.
    -- During migration this is the current instance's UUID.
    BEGIN
        _iid := current_setting('app.instance_id')::uuid;
    EXCEPTION WHEN OTHERS THEN
        -- Fallback: generate a random UUID (should not happen in normal flow)
        _iid := gen_random_uuid();
        RAISE WARNING 'app.instance_id not set, using generated UUID: %', _iid;
    END;

    -- Store in a temp table so the ALTER TABLE DEFAULT expressions can reference it
    CREATE TEMP TABLE _migration_instance_id (id UUID NOT NULL) ON COMMIT DROP;
    INSERT INTO _migration_instance_id VALUES (_iid);
END $$;

-- ============================================================================
-- Step 2: Add instance_id column to every table, backfill, set NOT NULL + DEFAULT
-- ============================================================================

-- Helper: We repeat this pattern per table. The DEFAULT uses
-- current_setting() so future INSERTs auto-populate from the session var.

-- projects
ALTER TABLE projects ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE projects SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE projects ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE projects ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_projects_instance_id ON projects(instance_id);

-- project_dependencies
ALTER TABLE project_dependencies ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE project_dependencies SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE project_dependencies ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE project_dependencies ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_project_dependencies_instance_id ON project_dependencies(instance_id);

-- tickets
ALTER TABLE tickets ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE tickets SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE tickets ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE tickets ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_tickets_instance_id ON tickets(instance_id);

-- change_requests
ALTER TABLE change_requests ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE change_requests SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE change_requests ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE change_requests ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_change_requests_instance_id ON change_requests(instance_id);

-- users
ALTER TABLE users ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE users SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE users ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE users ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_users_instance_id ON users(instance_id);

-- ticket_assignees
ALTER TABLE ticket_assignees ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE ticket_assignees SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE ticket_assignees ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE ticket_assignees ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_ticket_assignees_instance_id ON ticket_assignees(instance_id);

-- change_request_assignees
ALTER TABLE change_request_assignees ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE change_request_assignees SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE change_request_assignees ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE change_request_assignees ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_change_request_assignees_instance_id ON change_request_assignees(instance_id);

-- ticket_comments
ALTER TABLE ticket_comments ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE ticket_comments SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE ticket_comments ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE ticket_comments ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_ticket_comments_instance_id ON ticket_comments(instance_id);

-- change_request_comments
ALTER TABLE change_request_comments ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE change_request_comments SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE change_request_comments ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE change_request_comments ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_change_request_comments_instance_id ON change_request_comments(instance_id);

-- reviews
ALTER TABLE reviews ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE reviews SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE reviews ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE reviews ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_reviews_instance_id ON reviews(instance_id);

-- metadata
ALTER TABLE metadata ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE metadata SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE metadata ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE metadata ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_metadata_instance_id ON metadata(instance_id);

-- epic_summaries
ALTER TABLE epic_summaries ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE epic_summaries SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE epic_summaries ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE epic_summaries ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_epic_summaries_instance_id ON epic_summaries(instance_id);

-- epic_descriptions
ALTER TABLE epic_descriptions ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE epic_descriptions SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE epic_descriptions ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE epic_descriptions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_instance_id ON epic_descriptions(instance_id);

-- iteration_reports
ALTER TABLE iteration_reports ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE iteration_reports SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE iteration_reports ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE iteration_reports ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_iteration_reports_instance_id ON iteration_reports(instance_id);

-- phases
ALTER TABLE phases ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE phases SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE phases ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE phases ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_phases_instance_id ON phases(instance_id);

-- epics
ALTER TABLE epics ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE epics SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE epics ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE epics ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_epics_instance_id ON epics(instance_id);

-- plans
ALTER TABLE plans ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE plans SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE plans ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE plans ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_plans_instance_id ON plans(instance_id);

-- commits
ALTER TABLE commits ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE commits SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE commits ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE commits ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_commits_instance_id ON commits(instance_id);

-- commit_file_changes
ALTER TABLE commit_file_changes ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE commit_file_changes SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE commit_file_changes ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE commit_file_changes ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_commit_file_changes_instance_id ON commit_file_changes(instance_id);

-- branches
ALTER TABLE branches ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE branches SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE branches ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE branches ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_branches_instance_id ON branches(instance_id);

-- audit_log
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE audit_log SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE audit_log ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE audit_log ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_audit_log_instance_id ON audit_log(instance_id);

-- todos
ALTER TABLE todos ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE todos SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE todos ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE todos ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_todos_instance_id ON todos(instance_id);

-- tasks
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE tasks SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE tasks ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE tasks ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_tasks_instance_id ON tasks(instance_id);

-- action_history
ALTER TABLE action_history ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE action_history SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE action_history ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE action_history ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_action_history_instance_id ON action_history(instance_id);

-- memories
ALTER TABLE memories ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE memories SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE memories ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE memories ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_memories_instance_id ON memories(instance_id);

-- decisions
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE decisions SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE decisions ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE decisions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_decisions_instance_id ON decisions(instance_id);

-- background_jobs
ALTER TABLE background_jobs ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE background_jobs SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE background_jobs ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE background_jobs ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_background_jobs_instance_id ON background_jobs(instance_id);

-- parallel_sessions
ALTER TABLE parallel_sessions ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE parallel_sessions SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE parallel_sessions ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE parallel_sessions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_parallel_sessions_instance_id ON parallel_sessions(instance_id);

-- path_locks
ALTER TABLE path_locks ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE path_locks SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE path_locks ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE path_locks ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_path_locks_instance_id ON path_locks(instance_id);

-- specifications
ALTER TABLE specifications ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE specifications SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE specifications ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE specifications ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_specifications_instance_id ON specifications(instance_id);

-- goals
ALTER TABLE goals ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE goals SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE goals ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE goals ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_goals_instance_id ON goals(instance_id);

-- specification_sections
ALTER TABLE specification_sections ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE specification_sections SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE specification_sections ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE specification_sections ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_specification_sections_instance_id ON specification_sections(instance_id);

-- requirements
ALTER TABLE requirements ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE requirements SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE requirements ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_requirements_instance_id ON requirements(instance_id);

-- epic_requirements
ALTER TABLE epic_requirements ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE epic_requirements SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE epic_requirements ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE epic_requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_epic_requirements_instance_id ON epic_requirements(instance_id);

-- ticket_requirements
ALTER TABLE ticket_requirements ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE ticket_requirements SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE ticket_requirements ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE ticket_requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_ticket_requirements_instance_id ON ticket_requirements(instance_id);

-- state
ALTER TABLE state ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE state SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE state ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE state ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_state_instance_id ON state(instance_id);

-- iterations
ALTER TABLE iterations ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE iterations SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE iterations ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE iterations ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_iterations_instance_id ON iterations(instance_id);

-- history
ALTER TABLE history ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE history SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE history ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE history ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_history_instance_id ON history(instance_id);

-- handovers
ALTER TABLE handovers ADD COLUMN IF NOT EXISTS instance_id UUID;
UPDATE handovers SET instance_id = (SELECT id FROM _migration_instance_id) WHERE instance_id IS NULL;
ALTER TABLE handovers ALTER COLUMN instance_id SET NOT NULL;
ALTER TABLE handovers ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;
CREATE INDEX IF NOT EXISTS idx_handovers_instance_id ON handovers(instance_id);

-- ============================================================================
-- Step 3: Enable RLS + create isolation policies
-- ============================================================================
-- FORCE ROW LEVEL SECURITY ensures policies apply even to table owners,
-- which is critical for shared-database deployments where the app role
-- might own the tables.

ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE projects FORCE ROW LEVEL SECURITY;
CREATE POLICY projects_instance_isolation ON projects
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE project_dependencies ENABLE ROW LEVEL SECURITY;
ALTER TABLE project_dependencies FORCE ROW LEVEL SECURITY;
CREATE POLICY project_dependencies_instance_isolation ON project_dependencies
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY tickets_instance_isolation ON tickets
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE change_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE change_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY change_requests_instance_isolation ON change_requests
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE users FORCE ROW LEVEL SECURITY;
CREATE POLICY users_instance_isolation ON users
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE ticket_assignees ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_assignees FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_assignees_instance_isolation ON ticket_assignees
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE change_request_assignees ENABLE ROW LEVEL SECURITY;
ALTER TABLE change_request_assignees FORCE ROW LEVEL SECURITY;
CREATE POLICY change_request_assignees_instance_isolation ON change_request_assignees
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE ticket_comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_comments FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_comments_instance_isolation ON ticket_comments
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE change_request_comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE change_request_comments FORCE ROW LEVEL SECURITY;
CREATE POLICY change_request_comments_instance_isolation ON change_request_comments
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE reviews FORCE ROW LEVEL SECURITY;
CREATE POLICY reviews_instance_isolation ON reviews
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE metadata ENABLE ROW LEVEL SECURITY;
ALTER TABLE metadata FORCE ROW LEVEL SECURITY;
CREATE POLICY metadata_instance_isolation ON metadata
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE epic_summaries ENABLE ROW LEVEL SECURITY;
ALTER TABLE epic_summaries FORCE ROW LEVEL SECURITY;
CREATE POLICY epic_summaries_instance_isolation ON epic_summaries
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE epic_descriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE epic_descriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY epic_descriptions_instance_isolation ON epic_descriptions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE iteration_reports ENABLE ROW LEVEL SECURITY;
ALTER TABLE iteration_reports FORCE ROW LEVEL SECURITY;
CREATE POLICY iteration_reports_instance_isolation ON iteration_reports
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE phases ENABLE ROW LEVEL SECURITY;
ALTER TABLE phases FORCE ROW LEVEL SECURITY;
CREATE POLICY phases_instance_isolation ON phases
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE epics ENABLE ROW LEVEL SECURITY;
ALTER TABLE epics FORCE ROW LEVEL SECURITY;
CREATE POLICY epics_instance_isolation ON epics
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE plans ENABLE ROW LEVEL SECURITY;
ALTER TABLE plans FORCE ROW LEVEL SECURITY;
CREATE POLICY plans_instance_isolation ON plans
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE commits ENABLE ROW LEVEL SECURITY;
ALTER TABLE commits FORCE ROW LEVEL SECURITY;
CREATE POLICY commits_instance_isolation ON commits
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE commit_file_changes ENABLE ROW LEVEL SECURITY;
ALTER TABLE commit_file_changes FORCE ROW LEVEL SECURITY;
CREATE POLICY commit_file_changes_instance_isolation ON commit_file_changes
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE branches ENABLE ROW LEVEL SECURITY;
ALTER TABLE branches FORCE ROW LEVEL SECURITY;
CREATE POLICY branches_instance_isolation ON branches
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_log_instance_isolation ON audit_log
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE todos ENABLE ROW LEVEL SECURITY;
ALTER TABLE todos FORCE ROW LEVEL SECURITY;
CREATE POLICY todos_instance_isolation ON todos
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE tasks FORCE ROW LEVEL SECURITY;
CREATE POLICY tasks_instance_isolation ON tasks
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE action_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE action_history FORCE ROW LEVEL SECURITY;
CREATE POLICY action_history_instance_isolation ON action_history
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories FORCE ROW LEVEL SECURITY;
CREATE POLICY memories_instance_isolation ON memories
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE decisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE decisions FORCE ROW LEVEL SECURITY;
CREATE POLICY decisions_instance_isolation ON decisions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE background_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE background_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY background_jobs_instance_isolation ON background_jobs
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE parallel_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE parallel_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY parallel_sessions_instance_isolation ON parallel_sessions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE path_locks ENABLE ROW LEVEL SECURITY;
ALTER TABLE path_locks FORCE ROW LEVEL SECURITY;
CREATE POLICY path_locks_instance_isolation ON path_locks
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE specifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE specifications FORCE ROW LEVEL SECURITY;
CREATE POLICY specifications_instance_isolation ON specifications
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE goals ENABLE ROW LEVEL SECURITY;
ALTER TABLE goals FORCE ROW LEVEL SECURITY;
CREATE POLICY goals_instance_isolation ON goals
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE specification_sections ENABLE ROW LEVEL SECURITY;
ALTER TABLE specification_sections FORCE ROW LEVEL SECURITY;
CREATE POLICY specification_sections_instance_isolation ON specification_sections
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE requirements ENABLE ROW LEVEL SECURITY;
ALTER TABLE requirements FORCE ROW LEVEL SECURITY;
CREATE POLICY requirements_instance_isolation ON requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE epic_requirements ENABLE ROW LEVEL SECURITY;
ALTER TABLE epic_requirements FORCE ROW LEVEL SECURITY;
CREATE POLICY epic_requirements_instance_isolation ON epic_requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE ticket_requirements ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_requirements FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_requirements_instance_isolation ON ticket_requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE state ENABLE ROW LEVEL SECURITY;
ALTER TABLE state FORCE ROW LEVEL SECURITY;
CREATE POLICY state_instance_isolation ON state
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE iterations ENABLE ROW LEVEL SECURITY;
ALTER TABLE iterations FORCE ROW LEVEL SECURITY;
CREATE POLICY iterations_instance_isolation ON iterations
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE history ENABLE ROW LEVEL SECURITY;
ALTER TABLE history FORCE ROW LEVEL SECURITY;
CREATE POLICY history_instance_isolation ON history
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

ALTER TABLE handovers ENABLE ROW LEVEL SECURITY;
ALTER TABLE handovers FORCE ROW LEVEL SECURITY;
CREATE POLICY handovers_instance_isolation ON handovers
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);

-- ============================================================================
-- Step 4: Update UNIQUE constraints to be per-instance
-- ============================================================================
-- These constraints were previously global but must be scoped to instance_id
-- for multi-tenant isolation. We drop the old constraint and create a new one.

-- projects: name must be unique per instance
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_name_key;
CREATE UNIQUE INDEX IF NOT EXISTS projects_instance_name_unique ON projects(instance_id, name);

-- tickets: id_number and id_string must be unique per instance
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_id_number_key;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_instance_id_number_unique ON tickets(instance_id, id_number);
ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_id_string_key;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_instance_id_string_unique ON tickets(instance_id, id_string);

-- users: user_string, user_number, user_uuid must be unique per instance
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_user_string_key;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_string_unique ON users(instance_id, user_string);
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_user_number_key;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_number_unique ON users(instance_id, user_number);
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_user_uuid_key;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_uuid_unique ON users(instance_id, user_uuid);

-- epics: epic_number unique per instance
ALTER TABLE epics DROP CONSTRAINT IF EXISTS epics_epic_number_key;
CREATE UNIQUE INDEX IF NOT EXISTS epics_instance_epic_number_unique ON epics(instance_id, epic_number);

-- metadata: scope unique per instance+project
DROP INDEX IF EXISTS metadata_unique_scope;
CREATE UNIQUE INDEX metadata_unique_scope ON metadata(instance_id, project_id, key, COALESCE(scope_type, ''), COALESCE(scope_id, 0));

-- decisions: decision_number unique per instance+project
ALTER TABLE decisions DROP CONSTRAINT IF EXISTS decisions_project_id_decision_number_key;
CREATE UNIQUE INDEX IF NOT EXISTS decisions_instance_project_decision_unique ON decisions(instance_id, project_id, decision_number);

-- phases: phase_number unique per instance+project
ALTER TABLE phases DROP CONSTRAINT IF EXISTS phases_project_id_phase_number_key;
CREATE UNIQUE INDEX IF NOT EXISTS phases_instance_project_phase_unique ON phases(instance_id, project_id, phase_number);

-- plans: name unique per instance+project
ALTER TABLE plans DROP CONSTRAINT IF EXISTS plans_project_id_name_key;
CREATE UNIQUE INDEX IF NOT EXISTS plans_instance_project_name_unique ON plans(instance_id, project_id, name);

-- specifications: one per instance+project
DROP INDEX IF EXISTS idx_specifications_project_id;
CREATE UNIQUE INDEX idx_specifications_project_id ON specifications(instance_id, project_id);

-- goals: one per instance+project
DROP INDEX IF EXISTS idx_goals_project_id;
CREATE UNIQUE INDEX idx_goals_project_id ON goals(instance_id, project_id);

-- path_locks: active lock unique per instance+project+path
DROP INDEX IF EXISTS idx_path_locks_active_unique;
CREATE UNIQUE INDEX idx_path_locks_active_unique ON path_locks(instance_id, project_id, path) WHERE released_at IS NULL;
