ALTER TABLE memberships ADD COLUMN paused INTEGER NOT NULL DEFAULT 0 CHECK(paused IN (0,1));
UPDATE memberships SET payload=json_set(payload,'$.paused',json('false'));
