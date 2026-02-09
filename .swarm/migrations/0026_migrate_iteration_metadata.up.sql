-- Migrate iteration metadata to iterations table
-- This migration handles historical iterations that still have data in metadata (iter:NNN:*)

DO $$
DECLARE
    proj RECORD;
    meta_row RECORD;
    iter_num INTEGER;
    epic_label_val TEXT;
    phase_val TEXT;
    status_val TEXT;
    start_date_val TIMESTAMPTZ;
    end_date_val TIMESTAMPTZ;
    epic_id_val INTEGER;
    phase_id_val INTEGER;
BEGIN
    -- For each project
    FOR proj IN SELECT id FROM projects LOOP
        -- Find all iteration-scoped metadata (iter:NNN:*)
        FOR meta_row IN
            SELECT DISTINCT
                substring(key from 'iter:([0-9]+):') as iteration_number
            FROM metadata
            WHERE project_id = proj.id
              AND key LIKE 'iter:%:%'
              AND substring(key from 'iter:([0-9]+):') IS NOT NULL
        LOOP
            iter_num := meta_row.iteration_number::INTEGER;

            -- Skip if already exists in iterations table
            IF EXISTS (
                SELECT 1 FROM iterations
                WHERE project_id = proj.id
                  AND iteration_number = iter_num
            ) THEN
                CONTINUE;
            END IF;

            -- Get metadata fields for this iteration
            SELECT value INTO epic_label_val
            FROM metadata
            WHERE project_id = proj.id
              AND key = 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':epicLabel'
            LIMIT 1;

            SELECT value INTO phase_val
            FROM metadata
            WHERE project_id = proj.id
              AND key = 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':phase'
            LIMIT 1;

            SELECT value INTO status_val
            FROM metadata
            WHERE project_id = proj.id
              AND key = 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':status'
            LIMIT 1;

            SELECT value INTO start_date_val
            FROM metadata
            WHERE project_id = proj.id
              AND key = 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':startDate'
              AND value IS NOT NULL
              AND value != 'null'
              AND value != ''
            LIMIT 1;

            SELECT value INTO end_date_val
            FROM metadata
            WHERE project_id = proj.id
              AND key = 'iter:' || LPAD(iter_num::TEXT, 3, '0') || ':endDate'
              AND value IS NOT NULL
              AND value != 'null'
              AND value != ''
            LIMIT 1;

            -- Skip if missing critical fields
            IF epic_label_val IS NULL OR status_val IS NULL THEN
                CONTINUE;
            END IF;

            -- Resolve epic_id from epic_label
            SELECT id INTO epic_id_val
            FROM epics
            WHERE project_id = proj.id
              AND epic_label = epic_label_val
            LIMIT 1;

            -- Resolve phase_id from phase name
            IF phase_val IS NOT NULL THEN
                SELECT id INTO phase_id_val
                FROM phases
                WHERE project_id = proj.id
                  AND phase_name = phase_val
                LIMIT 1;
            END IF;

            -- Insert into iterations table
            INSERT INTO iterations (
                project_id,
                iteration_number,
                epic_id,
                phase_id,
                status,
                start_date,
                end_date,
                created_at,
                updated_at
            ) VALUES (
                proj.id,
                iter_num,
                epic_id_val,
                phase_id_val,
                status_val,
                COALESCE(start_date_val::TIMESTAMPTZ, NOW()),
                CASE WHEN end_date_val IS NOT NULL AND end_date_val != ''
                     THEN end_date_val::TIMESTAMPTZ
                     ELSE NULL
                END,
                NOW(),
                NOW()
            )
            ON CONFLICT (project_id, iteration_number) DO NOTHING;

        END LOOP;
    END LOOP;
END $$;

-- Optional: Clean up migrated iteration metadata (commented out for safety)
-- Uncomment if you want to remove old metadata after confirming migration success
-- DELETE FROM metadata WHERE key LIKE 'iter:%:%';
