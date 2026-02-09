-- Migration: Rename pull_requests to change_requests
-- This makes the terminology more workflow-agnostic (not just software development)

-- ============================================================================
-- Rename main table
-- ============================================================================

ALTER TABLE pull_requests RENAME TO change_requests;

-- Rename pr_type column to change_request_type
ALTER TABLE change_requests RENAME COLUMN pr_type TO change_request_type;

-- Update the constraint for change_request_type
ALTER TABLE change_requests DROP CONSTRAINT IF EXISTS pull_requests_pr_type_check;
ALTER TABLE change_requests ADD CONSTRAINT change_requests_change_request_type_check
    CHECK (change_request_type IN ('bug', 'feature', 'docs', 'refactor', 'test', 'chore'));

-- ============================================================================
-- Rename pr_assignees table
-- ============================================================================

ALTER TABLE pr_assignees RENAME TO change_request_assignees;
ALTER TABLE change_request_assignees RENAME COLUMN pr_number TO change_request_number;

-- ============================================================================
-- Update pr_comments table (renamed from comments in migration 8)
-- ============================================================================

ALTER TABLE pr_comments RENAME TO change_request_comments;
ALTER TABLE change_request_comments RENAME COLUMN pr_number TO change_request_number;

-- Update index
ALTER INDEX IF EXISTS idx_pr_comments_pr RENAME TO idx_change_request_comments_cr;
ALTER INDEX IF EXISTS idx_pr_comments_author RENAME TO idx_change_request_comments_author;
ALTER INDEX IF EXISTS idx_pr_comments_created_at RENAME TO idx_change_request_comments_created_at;

-- Update trigger
DROP TRIGGER IF EXISTS update_pr_comments_updated_at ON change_request_comments;
CREATE TRIGGER update_change_request_comments_updated_at BEFORE UPDATE ON change_request_comments
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ============================================================================
-- Update reviews table
-- ============================================================================

ALTER TABLE reviews RENAME COLUMN pr_number TO change_request_number;

-- ============================================================================
-- Rename indexes
-- ============================================================================

-- Main table indexes
ALTER INDEX IF EXISTS idx_prs_state RENAME TO idx_change_requests_state;
ALTER INDEX IF EXISTS idx_prs_type RENAME TO idx_change_requests_type;
ALTER INDEX IF EXISTS idx_prs_priority RENAME TO idx_change_requests_priority;
ALTER INDEX IF EXISTS idx_prs_epic_label RENAME TO idx_change_requests_epic_label;
ALTER INDEX IF EXISTS idx_prs_created_at RENAME TO idx_change_requests_created_at;

-- Assignees index
ALTER INDEX IF EXISTS idx_pr_assignees_user RENAME TO idx_change_request_assignees_user;

-- Reviews index
ALTER INDEX IF EXISTS idx_reviews_pr RENAME TO idx_reviews_change_request;

-- ============================================================================
-- Update trigger name
-- ============================================================================

DROP TRIGGER IF EXISTS update_prs_updated_at ON change_requests;
CREATE TRIGGER update_change_requests_updated_at BEFORE UPDATE ON change_requests
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
