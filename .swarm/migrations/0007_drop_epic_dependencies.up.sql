-- Migration: Drop epic dependencies, capabilities, and risks tables
-- These features are no longer used; plan ordering via position is sufficient

DROP TABLE IF EXISTS epic_dependencies;
DROP TABLE IF EXISTS epic_capabilities;
DROP TABLE IF EXISTS epic_risks;
