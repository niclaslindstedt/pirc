-- Rollback: Remove plans table and restore original epic structure

-- ============================================================================
-- Recreate project_plan_metadata table
-- ============================================================================

CREATE TABLE IF NOT EXISTS project_plan_metadata (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key VARCHAR(255) NOT NULL,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_project_plan_metadata_project_id ON project_plan_metadata(project_id);

-- Add trigger for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_project_plan_metadata_updated_at') THEN
        CREATE TRIGGER update_project_plan_metadata_updated_at BEFORE UPDATE ON project_plan_metadata
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- ============================================================================
-- Migrate data back from plans to project_plan_metadata
-- ============================================================================

INSERT INTO project_plan_metadata (project_id, key, value)
SELECT project_id, 'overview', overview
FROM plans
WHERE overview IS NOT NULL
ON CONFLICT (project_id, key) DO NOTHING;

INSERT INTO project_plan_metadata (project_id, key, value)
SELECT project_id, 'strategy', strategy
FROM plans
WHERE strategy IS NOT NULL
ON CONFLICT (project_id, key) DO NOTHING;

-- ============================================================================
-- Restore epics table structure
-- ============================================================================

-- Make phase_id required again (set to first phase of project if null)
UPDATE epics e
SET phase_id = (
    SELECT ph.id
    FROM phases ph
    JOIN plans pl ON pl.project_id = ph.project_id
    WHERE pl.id = e.plan_id
    ORDER BY ph.position
    LIMIT 1
)
WHERE e.phase_id IS NULL;

-- Remove plan_id column
ALTER TABLE epics DROP COLUMN IF EXISTS plan_id;

-- Make phase_id NOT NULL again
ALTER TABLE epics ALTER COLUMN phase_id SET NOT NULL;

-- ============================================================================
-- Drop plans table
-- ============================================================================

DROP TRIGGER IF EXISTS update_plans_updated_at ON plans;
DROP TABLE IF EXISTS plans;
