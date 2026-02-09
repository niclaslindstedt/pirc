-- Add tags column to decisions table for categorization
ALTER TABLE decisions ADD COLUMN tags TEXT[] DEFAULT '{}';

-- Create index for tag-based queries
CREATE INDEX idx_decisions_tags ON decisions USING GIN(tags);
