-- Seed state rows for projects missing them
-- Handles edge cases where projects were created after migration 0019
-- but before repository layer was updated

INSERT INTO state (project_id, created_at, updated_at)
SELECT id, NOW(), NOW()
FROM projects
WHERE NOT EXISTS (
    SELECT 1 FROM state WHERE state.project_id = projects.id
);
