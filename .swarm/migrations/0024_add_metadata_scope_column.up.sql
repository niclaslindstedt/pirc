-- Add scope_type and scope_id columns to metadata table
-- This enables better organization of metadata by scope (iteration, epic, ticket, phase)
-- Global metadata will have scope_type = NULL and scope_id = NULL

-- Drop the existing primary key constraint
ALTER TABLE metadata DROP CONSTRAINT metadata_pkey;

-- Add scope_type and scope_id columns (both nullable)
ALTER TABLE metadata ADD COLUMN scope_type VARCHAR(50);
ALTER TABLE metadata ADD COLUMN scope_id INTEGER;

-- Add a unique index that includes scope using expressions
-- This allows the same key to exist in different scopes
-- For global metadata (scope_type = NULL), we use COALESCE to treat NULL as a special value
-- Note: We use a unique index instead of PRIMARY KEY because PG doesn't allow expressions in PK
CREATE UNIQUE INDEX metadata_unique_scope ON metadata(project_id, key, COALESCE(scope_type, ''), COALESCE(scope_id, 0));

-- Add back a simple primary key on an id column for better ORM compatibility
ALTER TABLE metadata ADD COLUMN id SERIAL;
ALTER TABLE metadata ADD PRIMARY KEY (id);

-- Migrate existing data: parse keys and extract scope_type and scope_id
UPDATE metadata SET
  scope_type = CASE
    -- iteration pattern: iter:001: → type=iteration, id=1
    WHEN key ~ '^iter:\d+:' THEN 'iteration'

    -- epic pattern: epic:E001: → type=epic, extract numeric ID
    WHEN key ~ '^epic:E\d+:' THEN 'epic'
    WHEN key ~ '^epic:[^:]+:' THEN 'epic'

    -- ticket pattern: ticket:T\d+: → type=ticket, extract numeric ID
    WHEN key ~ '^ticket:T\d+:' THEN 'ticket'
    WHEN key ~ '^ticket:\d+:' THEN 'ticket'

    -- phase pattern: phase:\d+: → type=phase, id from number
    WHEN key ~ '^phase:\d+:' THEN 'phase'
    WHEN key ~ '^phase:[^:]+:' THEN 'phase'

    -- system pattern: system: → type=system (no ID)
    WHEN key ~ '^system:' THEN 'system'

    -- global (no prefix)
    ELSE NULL
  END,
  scope_id = CASE
    -- iteration: extract numeric part
    WHEN key ~ '^iter:(\d+):' THEN
      substring(key from '^iter:(\d+):')::INTEGER

    -- epic: extract numeric part (strip E prefix if present)
    WHEN key ~ '^epic:E(\d+):' THEN
      substring(key from '^epic:E(\d+):')::INTEGER
    WHEN key ~ '^epic:(\d+):' THEN
      substring(key from '^epic:(\d+):')::INTEGER

    -- ticket: extract numeric part (strip T prefix if present)
    WHEN key ~ '^ticket:T(\d+):' THEN
      substring(key from '^ticket:T(\d+):')::INTEGER
    WHEN key ~ '^ticket:(\d+):' THEN
      substring(key from '^ticket:(\d+):')::INTEGER

    -- phase: extract numeric part
    WHEN key ~ '^phase:(\d+):' THEN
      substring(key from '^phase:(\d+):')::INTEGER

    -- system or global: no ID
    ELSE NULL
  END,
  key = CASE
    -- Remove the scope prefix from key
    WHEN key ~ '^iter:\d+:' THEN
      substring(key from '^iter:\d+:(.*)$')

    WHEN key ~ '^epic:[^:]+:' THEN
      substring(key from '^epic:[^:]+:(.*)$')

    WHEN key ~ '^ticket:[^:]+:' THEN
      substring(key from '^ticket:[^:]+:(.*)$')

    WHEN key ~ '^phase:[^:]+:' THEN
      substring(key from '^phase:[^:]+:(.*)$')

    WHEN key ~ '^system:' THEN
      substring(key from '^system:(.*)$')

    -- No change for global
    ELSE key
  END;

-- Create index for efficient scope filtering
CREATE INDEX idx_metadata_scope_type ON metadata(project_id, scope_type);
CREATE INDEX idx_metadata_scope ON metadata(project_id, scope_type, scope_id);
