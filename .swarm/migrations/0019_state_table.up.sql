-- State Table Migration
-- Replaces built-in metadata fields with a typed, project-scoped state table

-- ============================================================================
-- State Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS state (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,

    -- Iteration tracking
    current_iteration INTEGER,

    -- Iteration state
    status VARCHAR(50),
    epic_label VARCHAR(255),
    phase VARCHAR(255),
    start_date TIMESTAMPTZ,
    end_date TIMESTAMPTZ,

    -- Current work items
    current_ticket VARCHAR(255),
    current_cr INTEGER,
    current_action VARCHAR(255),

    -- Review tracking
    last_reviewed_ticket VARCHAR(255),
    last_reviewed_cr INTEGER,

    -- Workflow tracking
    last_completed_action VARCHAR(255),

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_state_current_iteration ON state(current_iteration);
CREATE INDEX IF NOT EXISTS idx_state_status ON state(status);
CREATE INDEX IF NOT EXISTS idx_state_epic_label ON state(epic_label);
CREATE INDEX IF NOT EXISTS idx_state_phase ON state(phase);
CREATE INDEX IF NOT EXISTS idx_state_current_ticket ON state(current_ticket);
CREATE INDEX IF NOT EXISTS idx_state_current_cr ON state(current_cr);

-- Trigger for updated_at
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'update_state_updated_at') THEN
        CREATE TRIGGER update_state_updated_at BEFORE UPDATE ON state
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;

-- ============================================================================
-- Data Migration
-- ============================================================================

-- Create state records for all projects
INSERT INTO state (project_id, created_at, updated_at)
SELECT id, NOW(), NOW()
FROM projects
ON CONFLICT (project_id) DO NOTHING;

-- Populate current_iteration from system:currentiteration metadata
UPDATE state s
SET current_iteration = CAST(m.value AS INTEGER)
FROM metadata m
WHERE s.project_id = m.project_id
  AND m.key = 'system:currentiteration'
  AND m.value ~ '^\d+$';

-- Migrate iteration-scoped metadata fields to state table
DO $$
DECLARE
    proj RECORD;
    iter_num INTEGER;
    iter_key_prefix TEXT;
BEGIN
    FOR proj IN SELECT id FROM projects LOOP
        -- Get current iteration for this project
        SELECT CAST(value AS INTEGER) INTO iter_num
        FROM metadata
        WHERE project_id = proj.id
          AND key = 'system:currentiteration'
          AND value ~ '^\d+$'
        LIMIT 1;

        -- If we have a current iteration, populate state from metadata
        IF iter_num IS NOT NULL THEN
            iter_key_prefix := 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':';

            -- Update status
            UPDATE state
            SET status = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'status'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update epic_label
            UPDATE state
            SET epic_label = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'epiclabel'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update phase
            UPDATE state
            SET phase = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'phase'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update start_date
            UPDATE state
            SET start_date = (
                SELECT CAST(value AS TIMESTAMPTZ) FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'startdate'
                  AND value != 'null'
                  AND value != ''
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update end_date
            UPDATE state
            SET end_date = (
                SELECT CAST(value AS TIMESTAMPTZ) FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'enddate'
                  AND value != 'null'
                  AND value != ''
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update current_ticket
            UPDATE state
            SET current_ticket = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'currentticket'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update current_cr
            UPDATE state
            SET current_cr = (
                SELECT CAST(value AS INTEGER) FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'currentcr'
                  AND value ~ '^\d+$'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update current_action
            UPDATE state
            SET current_action = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'currentaction'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update last_reviewed_ticket
            UPDATE state
            SET last_reviewed_ticket = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'lastreviewedticket'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update last_reviewed_cr
            UPDATE state
            SET last_reviewed_cr = (
                SELECT CAST(value AS INTEGER) FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'lastreviewedcr'
                  AND value ~ '^\d+$'
                LIMIT 1
            )
            WHERE project_id = proj.id;

            -- Update last_completed_action
            UPDATE state
            SET last_completed_action = (
                SELECT value FROM metadata
                WHERE project_id = proj.id
                  AND key = iter_key_prefix || 'lastcompletedaction'
                LIMIT 1
            )
            WHERE project_id = proj.id;
        END IF;
    END LOOP;
END $$;
