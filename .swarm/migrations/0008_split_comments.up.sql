-- Split comments table into ticket_comments and pr_comments
-- This migration separates comment entities for cleaner data modeling

-- ============================================================================
-- Ticket Comments Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS ticket_comments (
    id SERIAL PRIMARY KEY,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ticket_comments_ticket ON ticket_comments(ticket_id);
CREATE INDEX IF NOT EXISTS idx_ticket_comments_author ON ticket_comments(author_id);
CREATE INDEX IF NOT EXISTS idx_ticket_comments_created_at ON ticket_comments(created_at);

-- ============================================================================
-- PR Comments Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS pr_comments (
    id SERIAL PRIMARY KEY,
    pr_number INTEGER NOT NULL REFERENCES pull_requests(number) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_pr_comments_pr ON pr_comments(pr_number);
CREATE INDEX IF NOT EXISTS idx_pr_comments_author ON pr_comments(author_id);
CREATE INDEX IF NOT EXISTS idx_pr_comments_created_at ON pr_comments(created_at);

-- ============================================================================
-- Migrate existing data
-- ============================================================================

-- Migrate ticket comments
INSERT INTO ticket_comments (id, ticket_id, author_id, body, created_at, updated_at)
SELECT id, ticket_id, author_id, body, created_at, created_at
FROM comments
WHERE ticket_id IS NOT NULL;

-- Migrate PR comments
INSERT INTO pr_comments (id, pr_number, author_id, body, created_at, updated_at)
SELECT id, pr_number, author_id, body, created_at, created_at
FROM comments
WHERE pr_number IS NOT NULL;

-- ============================================================================
-- Update sequences to continue from existing IDs
-- ============================================================================

SELECT setval('ticket_comments_id_seq', COALESCE((SELECT MAX(id) FROM ticket_comments), 0) + 1, false);
SELECT setval('pr_comments_id_seq', COALESCE((SELECT MAX(id) FROM pr_comments), 0) + 1, false);

-- ============================================================================
-- Drop old comments table
-- ============================================================================

DROP TABLE IF EXISTS comments;

-- ============================================================================
-- Add triggers for updated_at
-- ============================================================================

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_ticket_comments_updated_at') THEN
        CREATE TRIGGER update_ticket_comments_updated_at BEFORE UPDATE ON ticket_comments
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_pr_comments_updated_at') THEN
        CREATE TRIGGER update_pr_comments_updated_at BEFORE UPDATE ON pr_comments
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;
