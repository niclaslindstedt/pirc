-- Migration to make metadata keys case-insensitive by converting all keys to lowercase
-- This migration normalizes all existing metadata keys to lowercase for consistency

-- Step 1: Delete duplicate keys where the value is empty and a lowercase version exists with a value
-- This handles cases like (currentCR: empty) vs (currentcr: "67")
DELETE FROM metadata m1
WHERE m1.value = ''
  AND EXISTS (
    SELECT 1 FROM metadata m2
    WHERE m2.project_id = m1.project_id
      AND LOWER(m2.key) = LOWER(m1.key)
      AND m2.key != m1.key
      AND m2.value != ''
  );

-- Step 2: For remaining duplicates (both have values), delete the one that's NOT already lowercase
-- This prefers keeping the already-lowercase version
DELETE FROM metadata m1
WHERE m1.key != LOWER(m1.key)
  AND EXISTS (
    SELECT 1 FROM metadata m2
    WHERE m2.project_id = m1.project_id
      AND LOWER(m2.key) = LOWER(m1.key)
      AND m2.key != m1.key
      AND m2.key = LOWER(m2.key)
  );

-- Step 3: For any remaining duplicates (rare edge case), keep the first one alphabetically
DELETE FROM metadata m1
WHERE EXISTS (
    SELECT 1 FROM metadata m2
    WHERE m2.project_id = m1.project_id
      AND LOWER(m2.key) = LOWER(m1.key)
      AND m2.key != m1.key
      AND m2.key < m1.key
  );

-- Step 4: Now safe to convert all keys to lowercase
UPDATE metadata SET key = LOWER(key) WHERE key != LOWER(key);
