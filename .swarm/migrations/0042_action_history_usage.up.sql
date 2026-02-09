-- Add token usage and turns tracking to action_history
ALTER TABLE action_history ADD COLUMN input_tokens BIGINT;
ALTER TABLE action_history ADD COLUMN output_tokens BIGINT;
ALTER TABLE action_history ADD COLUMN num_turns INTEGER;
ALTER TABLE action_history ADD COLUMN cost_usd DOUBLE PRECISION;
