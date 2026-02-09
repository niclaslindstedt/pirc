-- Rollback: Restore original specifications structure
-- WARNING: Data from structured tables will be lost

-- Drop triggers first
DROP TRIGGER IF EXISTS trigger_requirements_updated_at ON requirements;
DROP TRIGGER IF EXISTS trigger_spec_sections_updated_at ON specification_sections;

-- Drop trigger functions
DROP FUNCTION IF EXISTS update_requirements_updated_at();
DROP FUNCTION IF EXISTS update_spec_sections_updated_at();

-- Drop junction tables
DROP TABLE IF EXISTS ticket_requirements;
DROP TABLE IF EXISTS epic_requirements;

-- Drop requirements table
DROP TABLE IF EXISTS requirements;

-- Drop sections table
DROP TABLE IF EXISTS specification_sections;

-- Restore original specifications table structure
ALTER TABLE specifications DROP COLUMN IF EXISTS title;
ALTER TABLE specifications DROP COLUMN IF EXISTS summary;
ALTER TABLE specifications ADD COLUMN IF NOT EXISTS content TEXT NOT NULL DEFAULT '';
