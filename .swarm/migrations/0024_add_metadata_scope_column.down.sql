-- Restore original key format (scope_type:scope_id:key) for downgrade
UPDATE metadata SET
  key = CASE
    WHEN scope_type = 'iteration' AND scope_id IS NOT NULL THEN
      'iter:' || LPAD(scope_id::TEXT, 3, '0') || ':' || key
    WHEN scope_type = 'epic' AND scope_id IS NOT NULL THEN
      'epic:E' || LPAD(scope_id::TEXT, 3, '0') || ':' || key
    WHEN scope_type = 'ticket' AND scope_id IS NOT NULL THEN
      'ticket:T' || LPAD(scope_id::TEXT, 3, '0') || ':' || key
    WHEN scope_type = 'phase' AND scope_id IS NOT NULL THEN
      'phase:' || scope_id || ':' || key
    WHEN scope_type = 'system' THEN
      'system:' || key
    ELSE key
  END;

-- Drop the id-based primary key
ALTER TABLE metadata DROP CONSTRAINT metadata_pkey;
ALTER TABLE metadata DROP COLUMN id;

-- Drop the unique scope index
DROP INDEX IF EXISTS metadata_unique_scope;

-- Drop indexes and columns
DROP INDEX IF EXISTS idx_metadata_scope;
DROP INDEX IF EXISTS idx_metadata_scope_type;
ALTER TABLE metadata DROP COLUMN scope_id;
ALTER TABLE metadata DROP COLUMN scope_type;

-- Restore original primary key
ALTER TABLE metadata ADD PRIMARY KEY (project_id, key);
