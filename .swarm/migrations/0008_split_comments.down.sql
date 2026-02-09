-- Revert: Merge ticket_comments and pr_comments back into single comments table

-- ============================================================================
-- Recreate unified comments table
-- ============================================================================

CREATE TABLE IF NOT EXISTS comments (
    id SERIAL PRIMARY KEY,
    ticket_id INTEGER REFERENCES tickets(id) ON DELETE CASCADE,
    pr_number INTEGER REFERENCES pull_requests(number) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT check_comment_target CHECK (
        (ticket_id IS NOT NULL AND pr_number IS NULL) OR
        (ticket_id IS NULL AND pr_number IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_comments_ticket ON comments(ticket_id);
CREATE INDEX IF NOT EXISTS idx_comments_pr ON comments(pr_number);
CREATE INDEX IF NOT EXISTS idx_comments_created_at ON comments(created_at);

-- ============================================================================
-- Migrate data back
-- ============================================================================

-- Migrate ticket comments back
INSERT INTO comments (id, ticket_id, pr_number, author_id, body, created_at)
SELECT id, ticket_id, NULL, author_id, body, created_at
FROM ticket_comments;

-- Migrate PR comments back
INSERT INTO comments (id, ticket_id, pr_number, author_id, body, created_at)
SELECT id, NULL, pr_number, author_id, body, created_at
FROM pr_comments;

-- ============================================================================
-- Update sequence
-- ============================================================================

SELECT setval('comments_id_seq', COALESCE((SELECT MAX(id) FROM comments), 0) + 1, false);

-- ============================================================================
-- Drop split tables
-- ============================================================================

DROP TABLE IF EXISTS ticket_comments;
DROP TABLE IF EXISTS pr_comments;
