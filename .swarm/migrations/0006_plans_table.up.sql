-- Migration: Add plans table as parent of epics
-- This restructures the hierarchy so:
--   PROJECT -> PHASES (milestones)
--   PROJECT -> PLANS (each with workflow type, contains epics)
--   EPIC -> belongs to PLAN, optionally scheduled in a PHASE

-- ============================================================================
-- Plans Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS plans (
    id SERIAL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    workflow_type VARCHAR(50) NOT NULL,
    overview TEXT,
    strategy TEXT,
    timeline TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'archived')),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, name)
);

CREATE INDEX IF NOT EXISTS idx_plans_project_id ON plans(project_id);
CREATE INDEX IF NOT EXISTS idx_plans_workflow_type ON plans(workflow_type);
CREATE INDEX IF NOT EXISTS idx_plans_status ON plans(status);
CREATE INDEX IF NOT EXISTS idx_plans_position ON plans(position);

-- Add trigger for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_plans_updated_at') THEN
        CREATE TRIGGER update_plans_updated_at BEFORE UPDATE ON plans
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- ============================================================================
-- Modify Epics Table
-- ============================================================================

-- Add plan_id column to epics
ALTER TABLE epics ADD COLUMN IF NOT EXISTS plan_id INTEGER REFERENCES plans(id) ON DELETE CASCADE;

-- Make phase_id nullable (now used for scheduling, not ownership)
ALTER TABLE epics ALTER COLUMN phase_id DROP NOT NULL;

-- Create index for plan_id
CREATE INDEX IF NOT EXISTS idx_epics_plan_id ON epics(plan_id);

-- ============================================================================
-- Data Migration
-- ============================================================================

-- Create a default plan for each existing project that has epics
-- This preserves existing data by creating a "main" plan
INSERT INTO plans (project_id, name, workflow_type, overview, strategy, status, position)
SELECT DISTINCT
    p.id,
    'main',
    p.workflow_type,
    (SELECT value FROM project_plan_metadata WHERE project_id = p.id AND key = 'overview'),
    (SELECT value FROM project_plan_metadata WHERE project_id = p.id AND key = 'strategy'),
    'active',
    0
FROM projects p
WHERE EXISTS (
    SELECT 1 FROM phases ph
    JOIN epics e ON e.phase_id = ph.id
    WHERE ph.project_id = p.id
)
ON CONFLICT (project_id, name) DO NOTHING;

-- Update existing epics to reference the default plan
UPDATE epics e
SET plan_id = (
    SELECT pl.id
    FROM plans pl
    JOIN phases ph ON ph.project_id = pl.project_id
    WHERE ph.id = e.phase_id
    AND pl.name = 'main'
    LIMIT 1
)
WHERE e.plan_id IS NULL;

-- ============================================================================
-- Drop old project_plan_metadata table (data migrated to plans)
-- ============================================================================

DROP TABLE IF EXISTS project_plan_metadata;
