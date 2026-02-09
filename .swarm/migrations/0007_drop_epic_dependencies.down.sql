-- Migration: Recreate epic dependencies, capabilities, and risks tables

CREATE TABLE IF NOT EXISTS epic_dependencies (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    depends_on_epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    dependency_type VARCHAR(50) DEFAULT 'blocks',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(epic_id, depends_on_epic_id),
    CHECK(epic_id != depends_on_epic_id)
);

CREATE INDEX IF NOT EXISTS idx_epic_dependencies_epic_id ON epic_dependencies(epic_id);
CREATE INDEX IF NOT EXISTS idx_epic_dependencies_depends_on ON epic_dependencies(depends_on_epic_id);

CREATE TABLE IF NOT EXISTS epic_capabilities (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    capability TEXT NOT NULL,
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_epic_capabilities_epic_id ON epic_capabilities(epic_id);

CREATE TABLE IF NOT EXISTS epic_risks (
    id SERIAL PRIMARY KEY,
    epic_id INTEGER NOT NULL REFERENCES epics(id) ON DELETE CASCADE,
    risk_description TEXT NOT NULL,
    risk_type VARCHAR(50),
    severity VARCHAR(20),
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_epic_risks_epic_id ON epic_risks(epic_id);
CREATE INDEX IF NOT EXISTS idx_epic_risks_risk_type ON epic_risks(risk_type);
CREATE INDEX IF NOT EXISTS idx_epic_risks_severity ON epic_risks(severity);
