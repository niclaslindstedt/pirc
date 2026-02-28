-- Migration 0048 (down): Revert workspace_id rename back to instance_id
--
-- Reverses migration 0048: renames workspace_id back to instance_id on all
-- tables, restores original indexes, RLS policies, DEFAULT expressions, and
-- unique constraints that reference instance_id / app.instance_id.

-- ============================================================================
-- Rename column back, restore indexes, RLS policies, and DEFAULT for each table
-- ============================================================================

-- projects
ALTER TABLE projects RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_projects_workspace_id;
CREATE INDEX IF NOT EXISTS idx_projects_instance_id ON projects(instance_id);
DROP POLICY IF EXISTS projects_workspace_isolation ON projects;
CREATE POLICY projects_instance_isolation ON projects
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE projects ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- project_dependencies
ALTER TABLE project_dependencies RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_project_dependencies_workspace_id;
CREATE INDEX IF NOT EXISTS idx_project_dependencies_instance_id ON project_dependencies(instance_id);
DROP POLICY IF EXISTS project_dependencies_workspace_isolation ON project_dependencies;
CREATE POLICY project_dependencies_instance_isolation ON project_dependencies
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE project_dependencies ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- tickets
ALTER TABLE tickets RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_tickets_workspace_id;
CREATE INDEX IF NOT EXISTS idx_tickets_instance_id ON tickets(instance_id);
DROP POLICY IF EXISTS tickets_workspace_isolation ON tickets;
CREATE POLICY tickets_instance_isolation ON tickets
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE tickets ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- change_requests
ALTER TABLE change_requests RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_change_requests_workspace_id;
CREATE INDEX IF NOT EXISTS idx_change_requests_instance_id ON change_requests(instance_id);
DROP POLICY IF EXISTS change_requests_workspace_isolation ON change_requests;
CREATE POLICY change_requests_instance_isolation ON change_requests
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE change_requests ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- users
ALTER TABLE users RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_users_workspace_id;
CREATE INDEX IF NOT EXISTS idx_users_instance_id ON users(instance_id);
DROP POLICY IF EXISTS users_workspace_isolation ON users;
CREATE POLICY users_instance_isolation ON users
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE users ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- ticket_assignees
ALTER TABLE ticket_assignees RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_ticket_assignees_workspace_id;
CREATE INDEX IF NOT EXISTS idx_ticket_assignees_instance_id ON ticket_assignees(instance_id);
DROP POLICY IF EXISTS ticket_assignees_workspace_isolation ON ticket_assignees;
CREATE POLICY ticket_assignees_instance_isolation ON ticket_assignees
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE ticket_assignees ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- change_request_assignees
ALTER TABLE change_request_assignees RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_change_request_assignees_workspace_id;
CREATE INDEX IF NOT EXISTS idx_change_request_assignees_instance_id ON change_request_assignees(instance_id);
DROP POLICY IF EXISTS change_request_assignees_workspace_isolation ON change_request_assignees;
CREATE POLICY change_request_assignees_instance_isolation ON change_request_assignees
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE change_request_assignees ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- ticket_comments
ALTER TABLE ticket_comments RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_ticket_comments_workspace_id;
CREATE INDEX IF NOT EXISTS idx_ticket_comments_instance_id ON ticket_comments(instance_id);
DROP POLICY IF EXISTS ticket_comments_workspace_isolation ON ticket_comments;
CREATE POLICY ticket_comments_instance_isolation ON ticket_comments
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE ticket_comments ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- change_request_comments
ALTER TABLE change_request_comments RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_change_request_comments_workspace_id;
CREATE INDEX IF NOT EXISTS idx_change_request_comments_instance_id ON change_request_comments(instance_id);
DROP POLICY IF EXISTS change_request_comments_workspace_isolation ON change_request_comments;
CREATE POLICY change_request_comments_instance_isolation ON change_request_comments
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE change_request_comments ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- reviews
ALTER TABLE reviews RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_reviews_workspace_id;
CREATE INDEX IF NOT EXISTS idx_reviews_instance_id ON reviews(instance_id);
DROP POLICY IF EXISTS reviews_workspace_isolation ON reviews;
CREATE POLICY reviews_instance_isolation ON reviews
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE reviews ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- metadata
ALTER TABLE metadata RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_metadata_workspace_id;
CREATE INDEX IF NOT EXISTS idx_metadata_instance_id ON metadata(instance_id);
DROP POLICY IF EXISTS metadata_workspace_isolation ON metadata;
CREATE POLICY metadata_instance_isolation ON metadata
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE metadata ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- epic_summaries
ALTER TABLE epic_summaries RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_epic_summaries_workspace_id;
CREATE INDEX IF NOT EXISTS idx_epic_summaries_instance_id ON epic_summaries(instance_id);
DROP POLICY IF EXISTS epic_summaries_workspace_isolation ON epic_summaries;
CREATE POLICY epic_summaries_instance_isolation ON epic_summaries
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE epic_summaries ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- epic_descriptions
ALTER TABLE epic_descriptions RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_epic_descriptions_workspace_id;
CREATE INDEX IF NOT EXISTS idx_epic_descriptions_instance_id ON epic_descriptions(instance_id);
DROP POLICY IF EXISTS epic_descriptions_workspace_isolation ON epic_descriptions;
CREATE POLICY epic_descriptions_instance_isolation ON epic_descriptions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE epic_descriptions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- iteration_reports
ALTER TABLE iteration_reports RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_iteration_reports_workspace_id;
CREATE INDEX IF NOT EXISTS idx_iteration_reports_instance_id ON iteration_reports(instance_id);
DROP POLICY IF EXISTS iteration_reports_workspace_isolation ON iteration_reports;
CREATE POLICY iteration_reports_instance_isolation ON iteration_reports
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE iteration_reports ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- phases
ALTER TABLE phases RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_phases_workspace_id;
CREATE INDEX IF NOT EXISTS idx_phases_instance_id ON phases(instance_id);
DROP POLICY IF EXISTS phases_workspace_isolation ON phases;
CREATE POLICY phases_instance_isolation ON phases
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE phases ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- epics
ALTER TABLE epics RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_epics_workspace_id;
CREATE INDEX IF NOT EXISTS idx_epics_instance_id ON epics(instance_id);
DROP POLICY IF EXISTS epics_workspace_isolation ON epics;
CREATE POLICY epics_instance_isolation ON epics
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE epics ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- plans
ALTER TABLE plans RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_plans_workspace_id;
CREATE INDEX IF NOT EXISTS idx_plans_instance_id ON plans(instance_id);
DROP POLICY IF EXISTS plans_workspace_isolation ON plans;
CREATE POLICY plans_instance_isolation ON plans
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE plans ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- commits
ALTER TABLE commits RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_commits_workspace_id;
CREATE INDEX IF NOT EXISTS idx_commits_instance_id ON commits(instance_id);
DROP POLICY IF EXISTS commits_workspace_isolation ON commits;
CREATE POLICY commits_instance_isolation ON commits
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE commits ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- commit_file_changes
ALTER TABLE commit_file_changes RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_commit_file_changes_workspace_id;
CREATE INDEX IF NOT EXISTS idx_commit_file_changes_instance_id ON commit_file_changes(instance_id);
DROP POLICY IF EXISTS commit_file_changes_workspace_isolation ON commit_file_changes;
CREATE POLICY commit_file_changes_instance_isolation ON commit_file_changes
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE commit_file_changes ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- branches
ALTER TABLE branches RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_branches_workspace_id;
CREATE INDEX IF NOT EXISTS idx_branches_instance_id ON branches(instance_id);
DROP POLICY IF EXISTS branches_workspace_isolation ON branches;
CREATE POLICY branches_instance_isolation ON branches
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE branches ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- audit_log
ALTER TABLE audit_log RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_audit_log_workspace_id;
CREATE INDEX IF NOT EXISTS idx_audit_log_instance_id ON audit_log(instance_id);
DROP POLICY IF EXISTS audit_log_workspace_isolation ON audit_log;
CREATE POLICY audit_log_instance_isolation ON audit_log
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE audit_log ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- todos
ALTER TABLE todos RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_todos_workspace_id;
CREATE INDEX IF NOT EXISTS idx_todos_instance_id ON todos(instance_id);
DROP POLICY IF EXISTS todos_workspace_isolation ON todos;
CREATE POLICY todos_instance_isolation ON todos
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE todos ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- tasks
ALTER TABLE tasks RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_tasks_workspace_id;
CREATE INDEX IF NOT EXISTS idx_tasks_instance_id ON tasks(instance_id);
DROP POLICY IF EXISTS tasks_workspace_isolation ON tasks;
CREATE POLICY tasks_instance_isolation ON tasks
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE tasks ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- action_history
ALTER TABLE action_history RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_action_history_workspace_id;
CREATE INDEX IF NOT EXISTS idx_action_history_instance_id ON action_history(instance_id);
DROP POLICY IF EXISTS action_history_workspace_isolation ON action_history;
CREATE POLICY action_history_instance_isolation ON action_history
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE action_history ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- memories
ALTER TABLE memories RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_memories_workspace_id;
CREATE INDEX IF NOT EXISTS idx_memories_instance_id ON memories(instance_id);
DROP POLICY IF EXISTS memories_workspace_isolation ON memories;
CREATE POLICY memories_instance_isolation ON memories
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE memories ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- decisions
ALTER TABLE decisions RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_decisions_workspace_id;
CREATE INDEX IF NOT EXISTS idx_decisions_instance_id ON decisions(instance_id);
DROP POLICY IF EXISTS decisions_workspace_isolation ON decisions;
CREATE POLICY decisions_instance_isolation ON decisions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE decisions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- background_jobs
ALTER TABLE background_jobs RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_background_jobs_workspace_id;
CREATE INDEX IF NOT EXISTS idx_background_jobs_instance_id ON background_jobs(instance_id);
DROP POLICY IF EXISTS background_jobs_workspace_isolation ON background_jobs;
CREATE POLICY background_jobs_instance_isolation ON background_jobs
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE background_jobs ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- parallel_sessions
ALTER TABLE parallel_sessions RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_parallel_sessions_workspace_id;
CREATE INDEX IF NOT EXISTS idx_parallel_sessions_instance_id ON parallel_sessions(instance_id);
DROP POLICY IF EXISTS parallel_sessions_workspace_isolation ON parallel_sessions;
CREATE POLICY parallel_sessions_instance_isolation ON parallel_sessions
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE parallel_sessions ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- path_locks
ALTER TABLE path_locks RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_path_locks_workspace_id;
CREATE INDEX IF NOT EXISTS idx_path_locks_instance_id ON path_locks(instance_id);
DROP POLICY IF EXISTS path_locks_workspace_isolation ON path_locks;
CREATE POLICY path_locks_instance_isolation ON path_locks
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE path_locks ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- specifications
ALTER TABLE specifications RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_specifications_workspace_id;
CREATE INDEX IF NOT EXISTS idx_specifications_instance_id ON specifications(instance_id);
DROP POLICY IF EXISTS specifications_workspace_isolation ON specifications;
CREATE POLICY specifications_instance_isolation ON specifications
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE specifications ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- goals
ALTER TABLE goals RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_goals_workspace_id;
CREATE INDEX IF NOT EXISTS idx_goals_instance_id ON goals(instance_id);
DROP POLICY IF EXISTS goals_workspace_isolation ON goals;
CREATE POLICY goals_instance_isolation ON goals
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE goals ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- specification_sections
ALTER TABLE specification_sections RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_specification_sections_workspace_id;
CREATE INDEX IF NOT EXISTS idx_specification_sections_instance_id ON specification_sections(instance_id);
DROP POLICY IF EXISTS specification_sections_workspace_isolation ON specification_sections;
CREATE POLICY specification_sections_instance_isolation ON specification_sections
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE specification_sections ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- requirements
ALTER TABLE requirements RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_requirements_workspace_id;
CREATE INDEX IF NOT EXISTS idx_requirements_instance_id ON requirements(instance_id);
DROP POLICY IF EXISTS requirements_workspace_isolation ON requirements;
CREATE POLICY requirements_instance_isolation ON requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- epic_requirements
ALTER TABLE epic_requirements RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_epic_requirements_workspace_id;
CREATE INDEX IF NOT EXISTS idx_epic_requirements_instance_id ON epic_requirements(instance_id);
DROP POLICY IF EXISTS epic_requirements_workspace_isolation ON epic_requirements;
CREATE POLICY epic_requirements_instance_isolation ON epic_requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE epic_requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- ticket_requirements
ALTER TABLE ticket_requirements RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_ticket_requirements_workspace_id;
CREATE INDEX IF NOT EXISTS idx_ticket_requirements_instance_id ON ticket_requirements(instance_id);
DROP POLICY IF EXISTS ticket_requirements_workspace_isolation ON ticket_requirements;
CREATE POLICY ticket_requirements_instance_isolation ON ticket_requirements
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE ticket_requirements ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- state
ALTER TABLE state RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_state_workspace_id;
CREATE INDEX IF NOT EXISTS idx_state_instance_id ON state(instance_id);
DROP POLICY IF EXISTS state_workspace_isolation ON state;
CREATE POLICY state_instance_isolation ON state
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE state ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- iterations
ALTER TABLE iterations RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_iterations_workspace_id;
CREATE INDEX IF NOT EXISTS idx_iterations_instance_id ON iterations(instance_id);
DROP POLICY IF EXISTS iterations_workspace_isolation ON iterations;
CREATE POLICY iterations_instance_isolation ON iterations
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE iterations ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- history
ALTER TABLE history RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_history_workspace_id;
CREATE INDEX IF NOT EXISTS idx_history_instance_id ON history(instance_id);
DROP POLICY IF EXISTS history_workspace_isolation ON history;
CREATE POLICY history_instance_isolation ON history
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE history ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- handovers
ALTER TABLE handovers RENAME COLUMN workspace_id TO instance_id;
DROP INDEX IF EXISTS idx_handovers_workspace_id;
CREATE INDEX IF NOT EXISTS idx_handovers_instance_id ON handovers(instance_id);
DROP POLICY IF EXISTS handovers_workspace_isolation ON handovers;
CREATE POLICY handovers_instance_isolation ON handovers
    USING (instance_id = current_setting('app.instance_id')::uuid)
    WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
ALTER TABLE handovers ALTER COLUMN instance_id SET DEFAULT current_setting('app.instance_id')::uuid;

-- ============================================================================
-- Restore UNIQUE constraints to reference instance_id
-- ============================================================================

-- projects: name must be unique per instance
DROP INDEX IF EXISTS projects_workspace_name_unique;
CREATE UNIQUE INDEX IF NOT EXISTS projects_instance_name_unique ON projects(instance_id, name);

-- tickets: id_number and id_string must be unique per instance
DROP INDEX IF EXISTS tickets_workspace_id_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_instance_id_number_unique ON tickets(instance_id, id_number);
DROP INDEX IF EXISTS tickets_workspace_id_string_unique;
CREATE UNIQUE INDEX IF NOT EXISTS tickets_instance_id_string_unique ON tickets(instance_id, id_string);

-- users: user_string, user_number, user_uuid must be unique per instance
DROP INDEX IF EXISTS users_workspace_user_string_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_string_unique ON users(instance_id, user_string);
DROP INDEX IF EXISTS users_workspace_user_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_number_unique ON users(instance_id, user_number);
DROP INDEX IF EXISTS users_workspace_user_uuid_unique;
CREATE UNIQUE INDEX IF NOT EXISTS users_instance_user_uuid_unique ON users(instance_id, user_uuid);

-- epics: epic_number unique per instance
DROP INDEX IF EXISTS epics_workspace_epic_number_unique;
CREATE UNIQUE INDEX IF NOT EXISTS epics_instance_epic_number_unique ON epics(instance_id, epic_number);

-- metadata: scope unique per instance+project
DROP INDEX IF EXISTS metadata_unique_scope;
CREATE UNIQUE INDEX metadata_unique_scope ON metadata(instance_id, project_id, key, COALESCE(scope_type, ''), COALESCE(scope_id, 0));

-- decisions: decision_number unique per instance+project
DROP INDEX IF EXISTS decisions_workspace_project_decision_unique;
CREATE UNIQUE INDEX IF NOT EXISTS decisions_instance_project_decision_unique ON decisions(instance_id, project_id, decision_number);

-- phases: phase_number unique per instance+project
DROP INDEX IF EXISTS phases_workspace_project_phase_unique;
CREATE UNIQUE INDEX IF NOT EXISTS phases_instance_project_phase_unique ON phases(instance_id, project_id, phase_number);

-- plans: name unique per instance+project
DROP INDEX IF EXISTS plans_workspace_project_name_unique;
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
