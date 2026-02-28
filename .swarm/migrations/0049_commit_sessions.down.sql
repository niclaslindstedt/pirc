-- Rollback migration 0049: Remove commit-session linkage

DROP TABLE IF EXISTS claude_sessions;
ALTER TABLE commits DROP COLUMN IF EXISTS claude_session_id;
