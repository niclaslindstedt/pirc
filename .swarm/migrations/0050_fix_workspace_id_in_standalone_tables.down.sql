-- Rollback migration 0050: Revert workspace_id back to instance_id in standalone tables

-- Revert standup_reports
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'standup_reports' AND column_name = 'workspace_id'
    ) THEN
        ALTER TABLE standup_reports RENAME COLUMN workspace_id TO instance_id;
        DROP INDEX IF EXISTS idx_standup_reports_workspace;
        CREATE INDEX IF NOT EXISTS idx_standup_reports_instance ON standup_reports(instance_id);
        DROP POLICY IF EXISTS standup_reports_workspace_policy ON standup_reports;
        CREATE POLICY standup_reports_instance_policy ON standup_reports
            USING (instance_id = current_setting('app.instance_id')::uuid);
        ALTER TABLE standup_reports ALTER COLUMN instance_id
            SET DEFAULT current_setting('app.instance_id')::uuid;
    END IF;
END $$;

-- Revert claude_sessions
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'claude_sessions' AND column_name = 'workspace_id'
    ) THEN
        ALTER TABLE claude_sessions RENAME COLUMN workspace_id TO instance_id;
        DROP INDEX IF EXISTS idx_claude_sessions_workspace;
        CREATE INDEX IF NOT EXISTS idx_claude_sessions_instance ON claude_sessions(instance_id);
        DROP POLICY IF EXISTS claude_sessions_workspace_isolation ON claude_sessions;
        CREATE POLICY claude_sessions_instance_isolation ON claude_sessions
            USING (instance_id = current_setting('app.instance_id')::uuid)
            WITH CHECK (instance_id = current_setting('app.instance_id')::uuid);
        ALTER TABLE claude_sessions ALTER COLUMN instance_id
            SET DEFAULT current_setting('app.instance_id')::uuid;
    END IF;
END $$;
