-- Revert renaming of current_phase back to phase

ALTER TABLE state DROP CONSTRAINT state_current_phase_fkey;
ALTER TABLE state ADD CONSTRAINT state_phase_id_fkey
    FOREIGN KEY (current_phase) REFERENCES phases(id) ON DELETE SET NULL;

ALTER INDEX idx_state_current_phase RENAME TO idx_state_phase;

ALTER TABLE state RENAME COLUMN current_phase TO phase;
