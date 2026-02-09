-- Rollback: Restore iteration metadata from iterations table
-- Note: This only restores basic iteration data, not work item state (current_ticket, etc.)

DO $$
DECLARE
    iter_row RECORD;
    epic_label_val TEXT;
    phase_name_val TEXT;
    iter_key_prefix TEXT;
BEGIN
    -- For each iteration in the iterations table
    FOR iter_row IN
        SELECT
            i.project_id,
            i.iteration_number,
            i.epic_id,
            i.phase_id,
            i.status,
            i.start_date,
            i.end_date,
            e.epic_label,
            p.phase_name
        FROM iterations i
        LEFT JOIN epics e ON i.epic_id = e.id
        LEFT JOIN phases p ON i.phase_id = p.id AND p.project_id = i.project_id
    LOOP
        iter_key_prefix := 'iter:' || LPAD(iter_row.iteration_number::TEXT, 3, '0') || ':';

        -- Restore epicLabel
        IF iter_row.epic_label IS NOT NULL THEN
            INSERT INTO metadata (project_id, key, value, created_at, updated_at)
            VALUES (
                iter_row.project_id,
                iter_key_prefix || 'epicLabel',
                iter_row.epic_label,
                NOW(),
                NOW()
            )
            ON CONFLICT (project_id, key) DO UPDATE
            SET value = EXCLUDED.value, updated_at = NOW();
        END IF;

        -- Restore phase
        IF iter_row.phase_name IS NOT NULL THEN
            INSERT INTO metadata (project_id, key, value, created_at, updated_at)
            VALUES (
                iter_row.project_id,
                iter_key_prefix || 'phase',
                iter_row.phase_name,
                NOW(),
                NOW()
            )
            ON CONFLICT (project_id, key) DO UPDATE
            SET value = EXCLUDED.value, updated_at = NOW();
        END IF;

        -- Restore status
        INSERT INTO metadata (project_id, key, value, created_at, updated_at)
        VALUES (
            iter_row.project_id,
            iter_key_prefix || 'status',
            iter_row.status,
            NOW(),
            NOW()
        )
        ON CONFLICT (project_id, key) DO UPDATE
        SET value = EXCLUDED.value, updated_at = NOW();

        -- Restore startDate
        INSERT INTO metadata (project_id, key, value, created_at, updated_at)
        VALUES (
            iter_row.project_id,
            iter_key_prefix || 'startDate',
            iter_row.start_date::TEXT,
            NOW(),
            NOW()
        )
        ON CONFLICT (project_id, key) DO UPDATE
        SET value = EXCLUDED.value, updated_at = NOW();

        -- Restore endDate (if exists)
        IF iter_row.end_date IS NOT NULL THEN
            INSERT INTO metadata (project_id, key, value, created_at, updated_at)
            VALUES (
                iter_row.project_id,
                iter_key_prefix || 'endDate',
                iter_row.end_date::TEXT,
                NOW(),
                NOW()
            )
            ON CONFLICT (project_id, key) DO UPDATE
            SET value = EXCLUDED.value, updated_at = NOW();
        END IF;

    END LOOP;
END $$;

-- Note: We don't delete from iterations table as this is just metadata restoration
-- To fully rollback, you would need to manually delete from iterations table if desired
