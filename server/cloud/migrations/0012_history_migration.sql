ALTER TABLE cloud_conversations ADD COLUMN crypto_history_complete boolean NOT NULL DEFAULT false;
ALTER TABLE cloud_conversations ADD COLUMN history_migration uuid;
ALTER TABLE cloud_messages ADD COLUMN history_migration jsonb, ADD COLUMN attachment_generation uuid;
ALTER TABLE cloud_contact_intents ADD COLUMN history_migration jsonb;
CREATE TABLE cloud_history_migrations (
 id uuid PRIMARY KEY, conversation uuid NOT NULL REFERENCES cloud_conversations(id), actor_id uuid NOT NULL REFERENCES accounts(id),
 conversation_revision bigint NOT NULL, source_digest text NOT NULL, backup_digest text NOT NULL, recovery_digest text NOT NULL,
 state text NOT NULL DEFAULT 'active' CHECK(state IN ('active','complete','conflicted')), verification_digest text,
 created_at timestamptz NOT NULL DEFAULT now(), completed_at timestamptz
);
CREATE TABLE cloud_history_items (
 migration_id uuid NOT NULL REFERENCES cloud_history_migrations(id), ordinal bigserial NOT NULL, kind text NOT NULL, item_id uuid NOT NULL,
 source_revision bigint NOT NULL, source_digest text NOT NULL, original_audience_revision text, converted boolean NOT NULL DEFAULT false,
 PRIMARY KEY(migration_id,kind,item_id), UNIQUE(migration_id,ordinal)
);
CREATE TABLE cloud_history_receipts (
 actor_id uuid NOT NULL REFERENCES accounts(id), request_id uuid NOT NULL, fingerprint text NOT NULL,
 migration_id uuid NOT NULL REFERENCES cloud_history_migrations(id),
 PRIMARY KEY(actor_id,request_id)
);
CREATE TABLE cloud_history_attachment_backups (
 migration_id uuid NOT NULL REFERENCES cloud_history_migrations(id), message_id uuid NOT NULL, generation uuid, metadata jsonb NOT NULL,
 PRIMARY KEY(migration_id,message_id)
);
