-- Migration 0048: Rename instance_id to workspace_id across all tables
--
-- Renames the tenant-isolation column from instance_id to workspace_id to
-- align with the "workspace" terminology used in the CLI (swarm workspaces).
-- Also updates all related indexes, RLS policies, DEFAULT expressions, and
-- unique constraints to use workspace_id / app.workspace_id throughout.

-- ============================================================================
-- Rename column, update indexes, RLS policies, and DEFAULT for each table
-- ============================================================================

-- projects
ALTER TABLE projects RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_projects_instance_id;
CREATE INDEX IF NOT EXISTS idx_projects_workspace_id ON projects(workspace_id);
DROP POLICY IF EXISTS projects_instance_isolation ON projects;
CREATE POLICY projects_workspace_isolation ON projects
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE projects ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- project_dependencies
ALTER TABLE project_dependencies RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_project_dependencies_instance_id;
CREATE INDEX IF NOT EXISTS idx_project_dependencies_workspace_id ON project_dependencies(workspace_id);
DROP POLICY IF EXISTS project_dependencies_instance_isolation ON project_dependencies;
CREATE POLICY project_dependencies_workspace_isolation ON project_dependencies
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE project_dependencies ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- tickets
ALTER TABLE tickets RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_tickets_instance_id;
CREATE INDEX IF NOT EXISTS idx_tickets_workspace_id ON tickets(workspace_id);
DROP POLICY IF EXISTS tickets_instance_isolation ON tickets;
CREATE POLICY tickets_workspace_isolation ON tickets
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE tickets ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- change_requests
ALTER TABLE change_requests RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_change_requests_instance_id;
CREATE INDEX IF NOT EXISTS idx_change_requests_workspace_id ON change_requests(workspace_id);
DROP POLICY IF EXISTS change_requests_instance_isolation ON change_requests;
CREATE POLICY change_requests_workspace_isolation ON change_requests
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE change_requests ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- users
ALTER TABLE users RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_users_instance_id;
CREATE INDEX IF NOT EXISTS idx_users_workspace_id ON users(workspace_id);
DROP POLICY IF EXISTS users_instance_isolation ON users;
CREATE POLICY users_workspace_isolation ON users
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE users ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- ticket_assignees
ALTER TABLE ticket_assignees RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_ticket_assignees_instance_id;
CREATE INDEX IF NOT EXISTS idx_ticket_assignees_workspace_id ON ticket_assignees(workspace_id);
DROP POLICY IF EXISTS ticket_assignees_instance_isolation ON ticket_assignees;
CREATE POLICY ticket_assignees_workspace_isolation ON ticket_assignees
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE ticket_assignees ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- change_request_assignees
ALTER TABLE change_request_assignees RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_change_request_assignees_instance_id;
CREATE INDEX IF NOT EXISTS idx_change_request_assignees_workspace_id ON change_request_assignees(workspace_id);
DROP POLICY IF EXISTS change_request_assignees_instance_isolation ON change_request_assignees;
CREATE POLICY change_request_assignees_workspace_isolation ON change_request_assignees
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE change_request_assignees ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- ticket_comments
ALTER TABLE ticket_comments RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_ticket_comments_instance_id;
CREATE INDEX IF NOT EXISTS idx_ticket_comments_workspace_id ON ticket_comments(workspace_id);
DROP POLICY IF EXISTS ticket_comments_instance_isolation ON ticket_comments;
CREATE POLICY ticket_comments_workspace_isolation ON ticket_comments
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE ticket_comments ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- change_request_comments
ALTER TABLE change_request_comments RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_change_request_comments_instance_id;
CREATE INDEX IF NOT EXISTS idx_change_request_comments_workspace_id ON change_request_comments(workspace_id);
DROP POLICY IF EXISTS change_request_comments_instance_isolation ON change_request_comments;
CREATE POLICY change_request_comments_workspace_isolation ON change_request_comments
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE change_request_comments ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- reviews
ALTER TABLE reviews RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_reviews_instance_id;
CREATE INDEX IF NOT EXISTS idx_reviews_workspace_id ON reviews(workspace_id);
DROP POLICY IF EXISTS reviews_instance_isolation ON reviews;
CREATE POLICY reviews_workspace_isolation ON reviews
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE reviews ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- metadata
ALTER TABLE metadata RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_metadata_instance_id;
CREATE INDEX IF NOT EXISTS idx_metadata_workspace_id ON metadata(workspace_id);
DROP POLICY IF EXISTS metadata_instance_isolation ON metadata;
CREATE POLICY metadata_workspace_isolation ON metadata
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE metadata ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- epic_summaries
ALTER TABLE epic_summaries RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_epic_summaries_instance_id;
CREATE INDEX IF NOT EXISTS idx_epic_summaries_workspace_id ON epic_summaries(workspace_id);
DROP POLICY IF EXISTS epic_summaries_instance_isolation ON epic_summaries;
CREATE POLICY epic_summaries_workspace_isolation ON epic_summaries
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE epic_summaries ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- epic_descriptions
ALTER TABLE epic_descriptions RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_epic_descriptions_instance_id;
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_workspace_id ON epic_descriptions(workspace_id);
DROP POLICY IF EXISTS epic_descriptions_instance_isolation ON epic_descriptions;
CREATE POLICY epic_descriptions_workspace_isolation ON epic_descriptions
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE epic_descriptions ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- iteration_reports
ALTER TABLE iteration_reports RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_iteration_reports_instance_id;
CREATE INDEX IF NOT EXISTS idx_iteration_reports_workspace_id ON iteration_reports(workspace_id);
DROP POLICY IF EXISTS iteration_reports_instance_isolation ON iteration_reports;
CREATE POLICY iteration_reports_workspace_isolation ON iteration_reports
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE iteration_reports ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- phases
ALTER TABLE phases RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_phases_instance_id;
CREATE INDEX IF NOT EXISTS idx_phases_workspace_id ON phases(workspace_id);
DROP POLICY IF EXISTS phases_instance_isolation ON phases;
CREATE POLICY phases_workspace_isolation ON phases
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE phases ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- epics
ALTER TABLE epics RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_epics_instance_id;
CREATE INDEX IF NOT EXISTS idx_epics_workspace_id ON epics(workspace_id);
DROP POLICY IF EXISTS epics_instance_isolation ON epics;
CREATE POLICY epics_workspace_isolation ON epics
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE epics ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- plans
ALTER TABLE plans RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_plans_instance_id;
CREATE INDEX IF NOT EXISTS idx_plans_workspace_id ON plans(workspace_id);
DROP POLICY IF EXISTS plans_instance_isolation ON plans;
CREATE POLICY plans_workspace_isolation ON plans
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE plans ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- commits
ALTER TABLE commits RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_commits_instance_id;
CREATE INDEX IF NOT EXISTS idx_commits_workspace_id ON commits(workspace_id);
DROP POLICY IF EXISTS commits_instance_isolation ON commits;
CREATE POLICY commits_workspace_isolation ON commits
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE commits ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- commit_file_changes
ALTER TABLE commit_file_changes RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_commit_file_changes_instance_id;
CREATE INDEX IF NOT EXISTS idx_commit_file_changes_workspace_id ON commit_file_changes(workspace_id);
DROP POLICY IF EXISTS commit_file_changes_instance_isolation ON commit_file_changes;
CREATE POLICY commit_file_changes_workspace_isolation ON commit_file_changes
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE commit_file_changes ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- branches
ALTER TABLE branches RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_branches_instance_id;
CREATE INDEX IF NOT EXISTS idx_branches_workspace_id ON branches(workspace_id);
DROP POLICY IF EXISTS branches_instance_isolation ON branches;
CREATE POLICY branches_workspace_isolation ON branches
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE branches ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- audit_log
ALTER TABLE audit_log RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_audit_log_instance_id;
CREATE INDEX IF NOT EXISTS idx_audit_log_workspace_id ON audit_log(workspace_id);
DROP POLICY IF EXISTS audit_log_instance_isolation ON audit_log;
CREATE POLICY audit_log_workspace_isolation ON audit_log
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE audit_log ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- todos
ALTER TABLE todos RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_todos_instance_id;
CREATE INDEX IF NOT EXISTS idx_todos_workspace_id ON todos(workspace_id);
DROP POLICY IF EXISTS todos_instance_isolation ON todos;
CREATE POLICY todos_workspace_isolation ON todos
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE todos ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- tasks
ALTER TABLE tasks RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_tasks_instance_id;
CREATE INDEX IF NOT EXISTS idx_tasks_workspace_id ON tasks(workspace_id);
DROP POLICY IF EXISTS tasks_instance_isolation ON tasks;
CREATE POLICY tasks_workspace_isolation ON tasks
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE tasks ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- action_history
ALTER TABLE action_history RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_action_history_instance_id;
CREATE INDEX IF NOT EXISTS idx_action_history_workspace_id ON action_history(workspace_id);
DROP POLICY IF EXISTS action_history_instance_isolation ON action_history;
CREATE POLICY action_history_workspace_isolation ON action_history
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE action_history ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- memories
ALTER TABLE memories RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_memories_instance_id;
CREATE INDEX IF NOT EXISTS idx_memories_workspace_id ON memories(workspace_id);
DROP POLICY IF EXISTS memories_instance_isolation ON memories;
CREATE POLICY memories_workspace_isolation ON memories
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE memories ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- decisions
ALTER TABLE decisions RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_decisions_instance_id;
CREATE INDEX IF NOT EXISTS idx_decisions_workspace_id ON decisions(workspace_id);
DROP POLICY IF EXISTS decisions_instance_isolation ON decisions;
CREATE POLICY decisions_workspace_isolation ON decisions
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE decisions ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- background_jobs
ALTER TABLE background_jobs RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_background_jobs_instance_id;
CREATE INDEX IF NOT EXISTS idx_background_jobs_workspace_id ON background_jobs(workspace_id);
DROP POLICY IF EXISTS background_jobs_instance_isolation ON background_jobs;
CREATE POLICY background_jobs_workspace_isolation ON background_jobs
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE background_jobs ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- parallel_sessions
ALTER TABLE parallel_sessions RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_parallel_sessions_instance_id;
CREATE INDEX IF NOT EXISTS idx_parallel_sessions_workspace_id ON parallel_sessions(workspace_id);
DROP POLICY IF EXISTS parallel_sessions_instance_isolation ON parallel_sessions;
CREATE POLICY parallel_sessions_workspace_isolation ON parallel_sessions
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE parallel_sessions ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- path_locks
ALTER TABLE path_locks RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_path_locks_instance_id;
CREATE INDEX IF NOT EXISTS idx_path_locks_workspace_id ON path_locks(workspace_id);
DROP POLICY IF EXISTS path_locks_instance_isolation ON path_locks;
CREATE POLICY path_locks_workspace_isolation ON path_locks
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE path_locks ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- specifications
ALTER TABLE specifications RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_specifications_instance_id;
CREATE INDEX IF NOT EXISTS idx_specifications_workspace_id ON specifications(workspace_id);
DROP POLICY IF EXISTS specifications_instance_isolation ON specifications;
CREATE POLICY specifications_workspace_isolation ON specifications
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE specifications ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- goals
ALTER TABLE goals RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_goals_instance_id;
CREATE INDEX IF NOT EXISTS idx_goals_workspace_id ON goals(workspace_id);
DROP POLICY IF EXISTS goals_instance_isolation ON goals;
CREATE POLICY goals_workspace_isolation ON goals
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE goals ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- specification_sections
ALTER TABLE specification_sections RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_specification_sections_instance_id;
CREATE INDEX IF NOT EXISTS idx_specification_sections_workspace_id ON specification_sections(workspace_id);
DROP POLICY IF EXISTS specification_sections_instance_isolation ON specification_sections;
CREATE POLICY specification_sections_workspace_isolation ON specification_sections
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE specification_sections ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- requirements
ALTER TABLE requirements RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_requirements_instance_id;
CREATE INDEX IF NOT EXISTS idx_requirements_workspace_id ON requirements(workspace_id);
DROP POLICY IF EXISTS requirements_instance_isolation ON requirements;
CREATE POLICY requirements_workspace_isolation ON requirements
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE requirements ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- epic_requirements
ALTER TABLE epic_requirements RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_epic_requirements_instance_id;
CREATE INDEX IF NOT EXISTS idx_epic_requirements_workspace_id ON epic_requirements(workspace_id);
DROP POLICY IF EXISTS epic_requirements_instance_isolation ON epic_requirements;
CREATE POLICY epic_requirements_workspace_isolation ON epic_requirements
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE epic_requirements ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- ticket_requirements
ALTER TABLE ticket_requirements RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_ticket_requirements_instance_id;
CREATE INDEX IF NOT EXISTS idx_ticket_requirements_workspace_id ON ticket_requirements(workspace_id);
DROP POLICY IF EXISTS ticket_requirements_instance_isolation ON ticket_requirements;
CREATE POLICY ticket_requirements_workspace_isolation ON ticket_requirements
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE ticket_requirements ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- state
ALTER TABLE state RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_state_instance_id;
CREATE INDEX IF NOT EXISTS idx_state_workspace_id ON state(workspace_id);
DROP POLICY IF EXISTS state_instance_isolation ON state;
CREATE POLICY state_workspace_isolation ON state
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE state ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- iterations
ALTER TABLE iterations RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_iterations_instance_id;
CREATE INDEX IF NOT EXISTS idx_iterations_workspace_id ON iterations(workspace_id);
DROP POLICY IF EXISTS iterations_instance_isolation ON iterations;
CREATE POLICY iterations_workspace_isolation ON iterations
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE iterations ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- history
ALTER TABLE history RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_history_instance_id;
CREATE INDEX IF NOT EXISTS idx_history_workspace_id ON history(workspace_id);
DROP POLICY IF EXISTS history_instance_isolation ON history;
CREATE POLICY history_workspace_isolation ON history
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE history ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- handovers
ALTER TABLE handovers RENAME COLUMN instance_id TO workspace_id;
DROP INDEX IF EXISTS idx_handovers_instance_id;
CREATE INDEX IF NOT EXISTS idx_handovers_workspace_id ON handovers(workspace_id);
DROP POLICY IF EXISTS handovers_instance_isolation ON handovers;
CREATE POLICY handovers_workspace_isolation ON handovers
    USING (workspace_id = current_setting('app.workspace_id')::uuid)
    WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
ALTER TABLE handovers ALTER COLUMN workspace_id SET DEFAULT current_setting('app.workspace_id')::uuid;

-- ============================================================================
-- Update UNIQUE constraints to reference workspace_id instead of instance_id
-- ============================================================================

-- projects: name must be unique per workspace
DROP INDEX IF EXISTS projects_instance_name_unique;
CREATE UNIQUE INDEX IF NOT EXISTS projects_workspace_name_unique ON projects(workspace_id, name);

-- tickets: id_number and id_string must be unique per workspace
DROP INDEX IF EXISTS tickets_instance_id_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_workspace_id_number_unique ON tickets(workspace_id, id_number);
DROP INDEX IF EXISTS tickets_instance_id_string_unique;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_workspace_id_string_unique ON tickets(workspace_id, id_string);

-- users: user_string, user_number, user_uuid must be unique per workspace
DROP INDEX IF EXISTS users_instance_user_string_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_workspace_user_string_unique ON users(workspace_id, user_string);
DROP INDEX IF EXISTS users_instance_user_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_workspace_user_number_unique ON users(workspace_id, user_number);
DROP INDEX IF EXISTS users_instance_user_uuid_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_workspace_user_uuid_unique ON users(workspace_id, user_uuid);

-- epics: epic_number unique per workspace
DROP INDEX IF EXISTS epics_instance_epic_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS epics_workspace_epic_number_unique ON epics(workspace_id, epic_number);

-- metadata: scope unique per workspace+project
DROP INDEX IF EXISTS metadata_unique_scope;
CREATE UNIQUE INDEX metadata_unique_scope ON metadata(workspace_id, project_id, key, COALESCE(scope_type, ''), COALESCE(scope_id, 0));

-- decisions: decision_number unique per workspace+project
DROP INDEX IF EXISTS decisions_instance_project_decision_unique;
CREATE UNIQUE INDEX IF NOT EXISTS decisions_workspace_project_decision_unique ON decisions(workspace_id, project_id, decision_number);

-- phases: phase_number unique per workspace+project
DROP INDEX IF EXISTS phases_instance_project_phase_unique;
CREATE UNIQUE INDEX IF NOT EXISTS phases_workspace_project_phase_unique ON phases(workspace_id, project_id, phase_number);

-- plans: name unique per workspace+project
DROP INDEX IF EXISTS plans_instance_project_name_unique;
CREATE UNIQUE INDEX IF NOT EXISTS plans_workspace_project_name_unique ON plans(workspace_id, project_id, name);

-- specifications: one per workspace+project
DROP INDEX IF EXISTS idx_specifications_project_id;
CREATE UNIQUE INDEX idx_specifications_project_id ON specifications(workspace_id, project_id);

-- goals: one per workspace+project
DROP INDEX IF EXISTS idx_goals_project_id;
CREATE UNIQUE INDEX idx_goals_project_id ON goals(workspace_id, project_id);

-- path_locks: active lock unique per workspace+project+path
DROP INDEX IF EXISTS idx_path_locks_active_unique;
CREATE UNIQUE INDEX idx_path_locks_active_unique ON path_locks(workspace_id, project_id, path) WHERE released_at IS NULL;
