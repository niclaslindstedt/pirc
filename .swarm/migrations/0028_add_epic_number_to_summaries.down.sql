-- Re-add epic_label column
ALTER TABLE epic_summaries ADD COLUMN IF NOT EXISTS epic_label VARCHAR(255);

-- Backfill epic_label from epics table based on epic_number
UPDATE epic_summaries es
SET epic_label = e.epic_label
FROM epics e
WHERE es.epic_number = e.epic_number;

-- Remove epic_number column
ALTER TABLE epic_summaries DROP COLUMN IF EXISTS epic_number;
