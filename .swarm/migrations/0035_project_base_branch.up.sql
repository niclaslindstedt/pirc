-- Add base_branch column to projects table
-- This allows each project to specify its own base branch for git operations

ALTER TABLE projects ADD COLUMN base_branch VARCHAR(255);

-- Set default to 'main' for existing projects to maintain backward compatibility
UPDATE projects SET base_branch = 'main' WHERE base_branch IS NULL;
