-- Migration 0050: Fix workspace_id column naming in standalone tables
--
-- Migrations 0046 (standup_reports) and 0049 (claude_sessions) were created
-- with instance_id/app.instance_id naming instead of workspace_id/app.workspace_id.
-- Migration 0048 renamed the column in other tables but missed these two.
-- This migration fixes existing databases that have the wrong column names.

-- Fix standup_reports (from 0046, missed by 0048)
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'standup_reports' AND column_name = 'instance_id'
    ) THEN
        ALTER TABLE standup_reports RENAME COLUMN instance_id TO workspace_id;
        DROP INDEX IF EXISTS idx_standup_reports_instance;
        CREATE INDEX IF NOT EXISTS idx_standup_reports_workspace ON standup_reports(workspace_id);
        DROP POLICY IF EXISTS standup_reports_instance_policy ON standup_reports;
        CREATE POLICY standup_reports_workspace_policy ON standup_reports
            USING (workspace_id = current_setting('app.workspace_id')::uuid);
        ALTER TABLE standup_reports ALTER COLUMN workspace_id
            SET DEFAULT current_setting('app.workspace_id')::uuid;
    END IF;
END $$;

-- Fix claude_sessions (from 0049)
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'claude_sessions' AND column_name = 'instance_id'
    ) THEN
        ALTER TABLE claude_sessions RENAME COLUMN instance_id TO workspace_id;
        DROP INDEX IF EXISTS idx_claude_sessions_instance;
        CREATE INDEX IF NOT EXISTS idx_claude_sessions_workspace ON claude_sessions(workspace_id);
        DROP POLICY IF EXISTS claude_sessions_instance_isolation ON claude_sessions;
        CREATE POLICY claude_sessions_workspace_isolation ON claude_sessions
            USING (workspace_id = current_setting('app.workspace_id')::uuid)
            WITH CHECK (workspace_id = current_setting('app.workspace_id')::uuid);
        ALTER TABLE claude_sessions ALTER COLUMN workspace_id
            SET DEFAULT current_setting('app.workspace_id')::uuid;
    END IF;
END $$;
