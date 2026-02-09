-- Add epic_number column to epic_summaries table
ALTER TABLE epic_summaries ADD COLUMN IF NOT EXISTS epic_number VARCHAR(50);

-- Backfill epic_number from epics table based on epic_label
UPDATE epic_summaries es
SET epic_number = e.epic_number
FROM epics e
WHERE es.epic_label = e.epic_label;

-- Drop epic_label column (replaced by epic_number)
ALTER TABLE epic_summaries DROP COLUMN IF EXISTS epic_label;
