-- Down migration for case-insensitive metadata
-- Note: This migration cannot fully restore original case, but restores common camelCase fields

-- Restore common iteration metadata fields to camelCase
UPDATE metadata SET key = REPLACE(key, 'epiclabel', 'epicLabel') WHERE key LIKE 'iter:%:epiclabel';
UPDATE metadata SET key = REPLACE(key, 'startdate', 'startDate') WHERE key LIKE 'iter:%:startdate';
UPDATE metadata SET key = REPLACE(key, 'enddate', 'endDate') WHERE key LIKE 'iter:%:enddate';
UPDATE metadata SET key = REPLACE(key, 'currentticket', 'currentTicket') WHERE key LIKE 'iter:%:currentticket';
UPDATE metadata SET key = REPLACE(key, 'currentcr', 'currentCR') WHERE key LIKE 'iter:%:currentcr';
UPDATE metadata SET key = REPLACE(key, 'lastreviewedticket', 'lastReviewedTicket') WHERE key LIKE 'iter:%:lastreviewedticket';
UPDATE metadata SET key = REPLACE(key, 'lastreviewedcr', 'lastReviewedCR') WHERE key LIKE 'iter:%:lastreviewedcr';

-- Restore common global metadata fields to camelCase
UPDATE metadata SET key = 'activeProjectId' WHERE key = 'activeprojectid';
