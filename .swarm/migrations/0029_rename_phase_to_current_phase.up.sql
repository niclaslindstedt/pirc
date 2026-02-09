-- Rename phase to current_phase for consistency with other current_* columns

ALTER TABLE state RENAME COLUMN phase TO current_phase;

-- Rename index
ALTER INDEX idx_state_phase RENAME TO idx_state_current_phase;

-- Rename foreign key constraint
ALTER TABLE state DROP CONSTRAINT state_phase_id_fkey;
ALTER TABLE state ADD CONSTRAINT state_current_phase_fkey
    FOREIGN KEY (current_phase) REFERENCES phases(id) ON DELETE SET NULL;
