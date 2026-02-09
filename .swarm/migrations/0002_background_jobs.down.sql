-- Rollback Background Jobs Tables

-- Remove the session_id column from background_jobs
ALTER TABLE background_jobs DROP COLUMN IF EXISTS session_id;

-- Drop parallel_sessions table
DROP TABLE IF EXISTS parallel_sessions CASCADE;

-- Drop background_jobs table
DROP TABLE IF EXISTS background_jobs CASCADE;
