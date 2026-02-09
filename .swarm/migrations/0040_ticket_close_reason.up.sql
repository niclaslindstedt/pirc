-- Add close_reason column to tickets table
-- This stores the reason why a ticket was closed (required when using --force)

ALTER TABLE tickets ADD COLUMN close_reason TEXT;
