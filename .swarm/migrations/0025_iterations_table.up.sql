-- Create iterations table to track all iterations (historical + current)
-- Replaces iteration data previously stored in metadata with iter: prefix

CREATE TABLE IF NOT EXISTS iterations (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    iteration_number INTEGER NOT NULL,
    epic_id INTEGER REFERENCES epics(id) ON DELETE SET NULL,
    phase_id INTEGER,
    status VARCHAR(50) NOT NULL DEFAULT 'in-progress',
    start_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    end_date TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, iteration_number)
);

-- Create index for faster lookups
CREATE INDEX IF NOT EXISTS idx_iterations_project_status
    ON iterations(project_id, status);

-- Migrate existing iteration from state table to iterations table
INSERT INTO iterations (
    project_id,
    iteration_number,
    epic_id,
    phase_id,
    status,
    start_date,
    end_date
)
SELECT
    project_id,
    current_iteration,
    current_epic,
    phase,
    status,
    start_date,
    end_date
FROM state
WHERE current_iteration IS NOT NULL
ON CONFLICT (project_id, iteration_number) DO NOTHING;
