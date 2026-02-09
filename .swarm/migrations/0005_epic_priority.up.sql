-- Add priority field to epics table
ALTER TABLE epics ADD COLUMN priority VARCHAR(20) DEFAULT 'medium' NOT NULL;

-- Create index for priority-based queries
CREATE INDEX IF NOT EXISTS idx_epics_priority ON epics(priority);

-- Add check constraint for valid priority values
ALTER TABLE epics ADD CONSTRAINT chk_epic_priority
    CHECK (priority IN ('critical', 'high', 'medium', 'low'));
