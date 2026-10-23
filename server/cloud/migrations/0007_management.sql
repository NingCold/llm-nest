ALTER TABLE accounts ADD COLUMN profile_revision bigint NOT NULL DEFAULT 1;
ALTER TABLE accounts ADD COLUMN administrators_revision bigint NOT NULL DEFAULT 1;
ALTER TABLE cloud_ai_bindings ADD COLUMN capabilities jsonb NOT NULL DEFAULT '[]';
