-- Migration: Restructure specifications for requirements traceability
-- This is a BREAKING CHANGE - existing specification content will be lost
-- The content column is dropped and replaced with structured tables

-- Step 1: Drop the old content column and add structured fields
ALTER TABLE specifications DROP COLUMN IF EXISTS content;
ALTER TABLE specifications ADD COLUMN IF NOT EXISTS title VARCHAR(500) NOT NULL DEFAULT '';
ALTER TABLE specifications ADD COLUMN IF NOT EXISTS summary TEXT;

-- Step 2: Create specification_sections table
CREATE TABLE IF NOT EXISTS specification_sections (
    id SERIAL PRIMARY KEY,
    specification_id INTEGER NOT NULL REFERENCES specifications(id) ON DELETE CASCADE,
    title VARCHAR(500) NOT NULL,
    body TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_spec_sections_spec_id ON specification_sections(specification_id);
CREATE INDEX IF NOT EXISTS idx_spec_sections_position ON specification_sections(specification_id, position);

-- Step 3: Create requirements table
CREATE TABLE IF NOT EXISTS requirements (
    id SERIAL PRIMARY KEY,
    section_id INTEGER NOT NULL REFERENCES specification_sections(id) ON DELETE CASCADE,
    req_number INTEGER NOT NULL,
    description TEXT NOT NULL,
    requirement_type VARCHAR(20) NOT NULL CHECK (requirement_type IN ('functional', 'non_functional', 'constraint')),
    priority VARCHAR(20) NOT NULL DEFAULT 'medium' CHECK (priority IN ('critical', 'high', 'medium', 'low')),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_requirements_section_id ON requirements(section_id);
CREATE INDEX IF NOT EXISTS idx_requirements_type ON requirements(requirement_type);
CREATE INDEX IF NOT EXISTS idx_requirements_priority ON requirements(priority);
CREATE INDEX IF NOT EXISTS idx_requirements_position ON requirements(section_id, position);

-- Step 4: Create epic_requirements junction table
CREATE TABLE IF NOT EXISTS epic_requirements (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    requirement_id INTEGER NOT NULL REFERENCES requirements(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(epic_id, requirement_id)
);

CREATE INDEX IF NOT EXISTS idx_epic_requirements_epic ON epic_requirements(epic_id);
CREATE INDEX IF NOT EXISTS idx_epic_requirements_req ON epic_requirements(requirement_id);

-- Step 5: Create ticket_requirements junction table
CREATE TABLE IF NOT EXISTS ticket_requirements (
    id SERIAL PRIMARY KEY,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    requirement_id INTEGER NOT NULL REFERENCES requirements(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(ticket_id, requirement_id)
);

CREATE INDEX IF NOT EXISTS idx_ticket_requirements_ticket ON ticket_requirements(ticket_id);
CREATE INDEX IF NOT EXISTS idx_ticket_requirements_req ON ticket_requirements(requirement_id);

-- Step 6: Add updated_at triggers for new tables
CREATE OR REPLACE FUNCTION update_spec_sections_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION update_requirements_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_spec_sections_updated_at ON specification_sections;
CREATE TRIGGER trigger_spec_sections_updated_at
    BEFORE UPDATE ON specification_sections
    FOR EACH ROW
    EXECUTE FUNCTION update_spec_sections_updated_at();

DROP TRIGGER IF EXISTS trigger_requirements_updated_at ON requirements;
CREATE TRIGGER trigger_requirements_updated_at
    BEFORE UPDATE ON requirements
    FOR EACH ROW
    EXECUTE FUNCTION update_requirements_updated_at();
