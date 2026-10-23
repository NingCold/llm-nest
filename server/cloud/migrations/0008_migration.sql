CREATE TABLE cloud_imports (
 id uuid PRIMARY KEY, owner uuid NOT NULL REFERENCES accounts(id), source_id uuid NOT NULL,
 snapshot_digest text NOT NULL, mapping jsonb NOT NULL, imported_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(owner,source_id)
);
ALTER TABLE cloud_messages ADD COLUMN import_origin jsonb;
ALTER TABLE cloud_conversations ADD COLUMN read_only boolean NOT NULL DEFAULT false;

CREATE TABLE cloud_import_reads (
 owner uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 message_id uuid NOT NULL REFERENCES cloud_messages(id) ON DELETE CASCADE,
 PRIMARY KEY(owner,message_id)
);
