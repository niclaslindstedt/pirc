-- Rollback Initial Swarm PostgreSQL Schema
-- This migration drops all tables created in the up migration

-- Drop all triggers first
DROP TRIGGER IF EXISTS update_tickets_updated_at ON tickets;
DROP TRIGGER IF EXISTS update_prs_updated_at ON pull_requests;
DROP TRIGGER IF EXISTS update_metadata_updated_at ON metadata;
DROP TRIGGER IF EXISTS update_epic_summaries_updated_at ON epic_summaries;
DROP TRIGGER IF EXISTS update_epic_descriptions_updated_at ON epic_descriptions;
DROP TRIGGER IF EXISTS update_iteration_reports_updated_at ON iteration_reports;
DROP TRIGGER IF EXISTS update_phases_updated_at ON phases;
DROP TRIGGER IF EXISTS update_epics_updated_at ON epics;
DROP TRIGGER IF EXISTS update_project_plan_metadata_updated_at ON project_plan_metadata;
DROP TRIGGER IF EXISTS update_todos_updated_at ON todos;
DROP TRIGGER IF EXISTS update_tasks_updated_at ON tasks;

-- Drop functions
DROP FUNCTION IF EXISTS update_updated_at_column();

-- Drop tables in reverse dependency order
DROP TABLE IF EXISTS action_history;
DROP TABLE IF EXISTS memories;
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS todos;
DROP TABLE IF EXISTS audit_log;
DROP TABLE IF EXISTS branches;
DROP TABLE IF EXISTS commit_file_changes;
DROP TABLE IF EXISTS commits;
DROP TABLE IF EXISTS epic_risks;
DROP TABLE IF EXISTS epic_dependencies;
DROP TABLE IF EXISTS epic_capabilities;
DROP TABLE IF EXISTS epics;
DROP TABLE IF EXISTS phases;
DROP TABLE IF EXISTS project_plan_metadata;
DROP TABLE IF EXISTS iteration_reports;
DROP TABLE IF EXISTS epic_descriptions;
DROP TABLE IF EXISTS epic_summaries;
DROP TABLE IF EXISTS metadata;
DROP TABLE IF EXISTS reviews;
DROP TABLE IF EXISTS comments;
DROP TABLE IF EXISTS pr_assignees;
DROP TABLE IF EXISTS ticket_assignees;
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS pull_requests;
DROP TABLE IF EXISTS tickets;
