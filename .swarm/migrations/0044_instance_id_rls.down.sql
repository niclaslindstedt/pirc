-- Migration 0044 DOWN: Remove instance_id column + Row-Level Security (RLS)
--
-- Reverses the RLS setup: drop policies, disable RLS, restore original
-- constraints, and remove the instance_id column from all tables.

-- ============================================================================
-- Step 1: Restore original UNIQUE constraints (before dropping instance_id)
-- ============================================================================

-- path_locks: restore original partial unique index
DROP INDEX IF EXISTS idx_path_locks_active_unique;
CREATE UNIQUE INDEX idx_path_locks_active_unique ON path_locks(project_id, path) WHERE released_at IS NULL;

-- goals: restore original unique index
DROP INDEX IF EXISTS idx_goals_project_id;
CREATE UNIQUE INDEX idx_goals_project_id ON goals(project_id);

-- specifications: restore original unique index
DROP INDEX IF EXISTS idx_specifications_project_id;
CREATE UNIQUE INDEX idx_specifications_project_id ON specifications(project_id);

-- plans: restore original constraint
DROP INDEX IF EXISTS plans_instance_project_name_unique;
ALTER TABLE plans ADD CONSTRAINT plans_project_id_name_key UNIQUE (project_id, name);

-- phases: restore original constraint
DROP INDEX IF EXISTS phases_instance_project_phase_unique;
ALTER TABLE phases ADD CONSTRAINT phases_project_id_phase_number_key UNIQUE (project_id, phase_number);

-- decisions: restore original constraint
DROP INDEX IF EXISTS decisions_instance_project_decision_unique;
ALTER TABLE decisions ADD CONSTRAINT decisions_project_id_decision_number_key UNIQUE (project_id, decision_number);

-- metadata: restore original unique index
DROP INDEX IF EXISTS metadata_unique_scope;
CREATE UNIQUE INDEX metadata_unique_scope ON metadata(project_id, key, COALESCE(scope_type, ''), COALESCE(scope_id, 0));

-- epics: restore original constraint
DROP INDEX IF EXISTS epics_instance_epic_number_unique;
ALTER TABLE epics ADD CONSTRAINT epics_epic_number_key UNIQUE (epic_number);

-- users: restore original constraints
DROP INDEX IF EXISTS users_instance_user_uuid_unique;
ALTER TABLE users ADD CONSTRAINT users_user_uuid_key UNIQUE (user_uuid);
DROP INDEX IF EXISTS users_instance_user_number_unique;
ALTER TABLE users ADD CONSTRAINT users_user_number_key UNIQUE (user_number);
DROP INDEX IF EXISTS users_instance_user_string_unique;
ALTER TABLE users ADD CONSTRAINT users_user_string_key UNIQUE (user_string);

-- tickets: restore original constraints
DROP INDEX IF EXISTS tickets_instance_id_string_unique;
ALTER TABLE tickets ADD CONSTRAINT tickets_id_string_key UNIQUE (id_string);
DROP INDEX IF EXISTS tickets_instance_id_number_unique;
ALTER TABLE tickets ADD CONSTRAINT tickets_id_number_key UNIQUE (id_number);

-- projects: restore original constraint
DROP INDEX IF EXISTS projects_instance_name_unique;
ALTER TABLE projects ADD CONSTRAINT projects_name_key UNIQUE (name);

-- ============================================================================
-- Step 2: Drop RLS policies and disable RLS
-- ============================================================================

DROP POLICY IF EXISTS projects_instance_isolation ON projects;
ALTER TABLE projects DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS project_dependencies_instance_isolation ON project_dependencies;
ALTER TABLE project_dependencies DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tickets_instance_isolation ON tickets;
ALTER TABLE tickets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS change_requests_instance_isolation ON change_requests;
ALTER TABLE change_requests DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS users_instance_isolation ON users;
ALTER TABLE users DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS ticket_assignees_instance_isolation ON ticket_assignees;
ALTER TABLE ticket_assignees DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS change_request_assignees_instance_isolation ON change_request_assignees;
ALTER TABLE change_request_assignees DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS ticket_comments_instance_isolation ON ticket_comments;
ALTER TABLE ticket_comments DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS change_request_comments_instance_isolation ON change_request_comments;
ALTER TABLE change_request_comments DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS reviews_instance_isolation ON reviews;
ALTER TABLE reviews DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS metadata_instance_isolation ON metadata;
ALTER TABLE metadata DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS epic_summaries_instance_isolation ON epic_summaries;
ALTER TABLE epic_summaries DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS epic_descriptions_instance_isolation ON epic_descriptions;
ALTER TABLE epic_descriptions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS iteration_reports_instance_isolation ON iteration_reports;
ALTER TABLE iteration_reports DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS phases_instance_isolation ON phases;
ALTER TABLE phases DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS epics_instance_isolation ON epics;
ALTER TABLE epics DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS plans_instance_isolation ON plans;
ALTER TABLE plans DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS commits_instance_isolation ON commits;
ALTER TABLE commits DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS commit_file_changes_instance_isolation ON commit_file_changes;
ALTER TABLE commit_file_changes DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS branches_instance_isolation ON branches;
ALTER TABLE branches DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS audit_log_instance_isolation ON audit_log;
ALTER TABLE audit_log DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS todos_instance_isolation ON todos;
ALTER TABLE todos DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tasks_instance_isolation ON tasks;
ALTER TABLE tasks DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS action_history_instance_isolation ON action_history;
ALTER TABLE action_history DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS memories_instance_isolation ON memories;
ALTER TABLE memories DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS decisions_instance_isolation ON decisions;
ALTER TABLE decisions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS background_jobs_instance_isolation ON background_jobs;
ALTER TABLE background_jobs DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS parallel_sessions_instance_isolation ON parallel_sessions;
ALTER TABLE parallel_sessions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS path_locks_instance_isolation ON path_locks;
ALTER TABLE path_locks DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS specifications_instance_isolation ON specifications;
ALTER TABLE specifications DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS goals_instance_isolation ON goals;
ALTER TABLE goals DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS specification_sections_instance_isolation ON specification_sections;
ALTER TABLE specification_sections DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS requirements_instance_isolation ON requirements;
ALTER TABLE requirements DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS epic_requirements_instance_isolation ON epic_requirements;
ALTER TABLE epic_requirements DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS ticket_requirements_instance_isolation ON ticket_requirements;
ALTER TABLE ticket_requirements DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS state_instance_isolation ON state;
ALTER TABLE state DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS iterations_instance_isolation ON iterations;
ALTER TABLE iterations DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS history_instance_isolation ON history;
ALTER TABLE history DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS handovers_instance_isolation ON handovers;
ALTER TABLE handovers DISABLE ROW LEVEL SECURITY;

-- ============================================================================
-- Step 3: Drop instance_id columns and indexes
-- ============================================================================

ALTER TABLE projects DROP COLUMN IF EXISTS instance_id;
ALTER TABLE project_dependencies DROP COLUMN IF EXISTS instance_id;
ALTER TABLE tickets DROP COLUMN IF EXISTS instance_id;
ALTER TABLE change_requests DROP COLUMN IF EXISTS instance_id;
ALTER TABLE users DROP COLUMN IF EXISTS instance_id;
ALTER TABLE ticket_assignees DROP COLUMN IF EXISTS instance_id;
ALTER TABLE change_request_assignees DROP COLUMN IF EXISTS instance_id;
ALTER TABLE ticket_comments DROP COLUMN IF EXISTS instance_id;
ALTER TABLE change_request_comments DROP COLUMN IF EXISTS instance_id;
ALTER TABLE reviews DROP COLUMN IF EXISTS instance_id;
ALTER TABLE metadata DROP COLUMN IF EXISTS instance_id;
ALTER TABLE epic_summaries DROP COLUMN IF EXISTS instance_id;
ALTER TABLE epic_descriptions DROP COLUMN IF EXISTS instance_id;
ALTER TABLE iteration_reports DROP COLUMN IF EXISTS instance_id;
ALTER TABLE phases DROP COLUMN IF EXISTS instance_id;
ALTER TABLE epics DROP COLUMN IF EXISTS instance_id;
ALTER TABLE plans DROP COLUMN IF EXISTS instance_id;
ALTER TABLE commits DROP COLUMN IF EXISTS instance_id;
ALTER TABLE commit_file_changes DROP COLUMN IF EXISTS instance_id;
ALTER TABLE branches DROP COLUMN IF EXISTS instance_id;
ALTER TABLE audit_log DROP COLUMN IF EXISTS instance_id;
ALTER TABLE todos DROP COLUMN IF EXISTS instance_id;
ALTER TABLE tasks DROP COLUMN IF EXISTS instance_id;
ALTER TABLE action_history DROP COLUMN IF EXISTS instance_id;
ALTER TABLE memories DROP COLUMN IF EXISTS instance_id;
ALTER TABLE decisions DROP COLUMN IF EXISTS instance_id;
ALTER TABLE background_jobs DROP COLUMN IF EXISTS instance_id;
ALTER TABLE parallel_sessions DROP COLUMN IF EXISTS instance_id;
ALTER TABLE path_locks DROP COLUMN IF EXISTS instance_id;
ALTER TABLE specifications DROP COLUMN IF EXISTS instance_id;
ALTER TABLE goals DROP COLUMN IF EXISTS instance_id;
ALTER TABLE specification_sections DROP COLUMN IF EXISTS instance_id;
ALTER TABLE requirements DROP COLUMN IF EXISTS instance_id;
ALTER TABLE epic_requirements DROP COLUMN IF EXISTS instance_id;
ALTER TABLE ticket_requirements DROP COLUMN IF EXISTS instance_id;
ALTER TABLE state DROP COLUMN IF EXISTS instance_id;
ALTER TABLE iterations DROP COLUMN IF EXISTS instance_id;
ALTER TABLE history DROP COLUMN IF EXISTS instance_id;
ALTER TABLE handovers DROP COLUMN IF EXISTS instance_id;
