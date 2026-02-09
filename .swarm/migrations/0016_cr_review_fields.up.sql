-- Add review fields to change_requests table
--
-- This migration adds review status, message, and timestamp directly to the
-- change_requests table to enable a more reliable CR review workflow. This moves
-- away from using metadata and aliases for tracking review state.

ALTER TABLE change_requests
    ADD COLUMN review_status VARCHAR(20) CHECK (review_status IN ('pending', 'approved', 'denied')),
    ADD COLUMN review_message TEXT,
    ADD COLUMN reviewed_at TIMESTAMPTZ;

-- Create index for review status to optimize queries
CREATE INDEX IF NOT EXISTS idx_change_requests_review_status ON change_requests(review_status);

-- Create index for reviewed_at to support time-based queries
CREATE INDEX IF NOT EXISTS idx_change_requests_reviewed_at ON change_requests(reviewed_at);
