-- Rollback: Rename change_requests back to pull_requests

-- ============================================================================
-- Restore trigger name
-- ============================================================================

DROP TRIGGER IF EXISTS update_change_requests_updated_at ON change_requests;
CREATE TRIGGER update_prs_updated_at BEFORE UPDATE ON change_requests
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ============================================================================
-- Restore indexes
-- ============================================================================

-- Main table indexes
ALTER INDEX IF EXISTS idx_change_requests_state RENAME TO idx_prs_state;
ALTER INDEX IF EXISTS idx_change_requests_type RENAME TO idx_prs_type;
ALTER INDEX IF EXISTS idx_change_requests_priority RENAME TO idx_prs_priority;
ALTER INDEX IF EXISTS idx_change_requests_epic_label RENAME TO idx_prs_epic_label;
ALTER INDEX IF EXISTS idx_change_requests_created_at RENAME TO idx_prs_created_at;

-- Assignees index
ALTER INDEX IF EXISTS idx_change_request_assignees_user RENAME TO idx_pr_assignees_user;

-- Reviews index
ALTER INDEX IF EXISTS idx_reviews_change_request RENAME TO idx_reviews_pr;

-- ============================================================================
-- Restore reviews table
-- ============================================================================

ALTER TABLE reviews RENAME COLUMN change_request_number TO pr_number;

-- ============================================================================
-- Restore pr_comments table (from change_request_comments)
-- ============================================================================

-- Restore trigger
DROP TRIGGER IF EXISTS update_change_request_comments_updated_at ON change_request_comments;
CREATE TRIGGER update_pr_comments_updated_at BEFORE UPDATE ON change_request_comments
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Restore indexes
ALTER INDEX IF EXISTS idx_change_request_comments_cr RENAME TO idx_pr_comments_pr;
ALTER INDEX IF EXISTS idx_change_request_comments_author RENAME TO idx_pr_comments_author;
ALTER INDEX IF EXISTS idx_change_request_comments_created_at RENAME TO idx_pr_comments_created_at;

-- Rename column and table
ALTER TABLE change_request_comments RENAME COLUMN change_request_number TO pr_number;
ALTER TABLE change_request_comments RENAME TO pr_comments;

-- ============================================================================
-- Restore pr_assignees table
-- ============================================================================

ALTER TABLE change_request_assignees RENAME COLUMN change_request_number TO pr_number;
ALTER TABLE change_request_assignees RENAME TO pr_assignees;

-- ============================================================================
-- Restore main table
-- ============================================================================

ALTER TABLE change_requests DROP CONSTRAINT IF EXISTS change_requests_change_request_type_check;
ALTER TABLE change_requests RENAME COLUMN change_request_type TO pr_type;
ALTER TABLE change_requests ADD CONSTRAINT pull_requests_pr_type_check
    CHECK (pr_type IN ('bug', 'feature', 'docs', 'refactor', 'test', 'chore'));

ALTER TABLE change_requests RENAME TO pull_requests;
