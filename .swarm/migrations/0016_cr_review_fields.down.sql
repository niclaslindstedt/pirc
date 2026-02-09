-- Remove review fields from change_requests table

DROP INDEX IF EXISTS idx_change_requests_reviewed_at;
DROP INDEX IF EXISTS idx_change_requests_review_status;

ALTER TABLE change_requests
    DROP COLUMN IF EXISTS reviewed_at,
    DROP COLUMN IF EXISTS review_message,
    DROP COLUMN IF EXISTS review_status;
