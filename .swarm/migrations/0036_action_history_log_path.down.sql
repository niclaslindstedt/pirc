-- Remove log_file_path column from action_history table

ALTER TABLE action_history DROP COLUMN log_file_path;
