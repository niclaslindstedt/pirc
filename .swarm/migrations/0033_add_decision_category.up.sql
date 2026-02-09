-- Add category column to decisions table (required, single value per decision)
-- Categories group decisions for organizational purposes (e.g., "architecture", "api", "database")
ALTER TABLE decisions ADD COLUMN category VARCHAR(255) NOT NULL DEFAULT 'uncategorized';

-- Create index for category-based queries
CREATE INDEX idx_decisions_category ON decisions(category);
