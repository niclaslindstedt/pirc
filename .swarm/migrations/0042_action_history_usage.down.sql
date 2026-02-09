-- Remove token usage and turns tracking from action_history
ALTER TABLE action_history DROP COLUMN IF EXISTS input_tokens;
ALTER TABLE action_history DROP COLUMN IF EXISTS output_tokens;
ALTER TABLE action_history DROP COLUMN IF EXISTS num_turns;
ALTER TABLE action_history DROP COLUMN IF EXISTS cost_usd;
