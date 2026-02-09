-- Add log_file_path column to action_history table
-- This stores the path to the log file captured during action execution

ALTER TABLE action_history ADD COLUMN log_file_path VARCHAR(512);
