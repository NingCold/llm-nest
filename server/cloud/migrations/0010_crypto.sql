-- Content protection is performed only by authorized endpoints. These tables contain
-- public device keys, opaque encrypted payloads and routing/version metadata.
ALTER TABLE cloud_conversations ADD COLUMN crypto_enabled boolean NOT NULL DEFAULT false;
ALTER TABLE cloud_conversations ADD COLUMN crypto_from_position bigint;
ALTER TABLE cloud_messages ADD COLUMN sealed jsonb;
ALTER TABLE cloud_messages ADD CONSTRAINT cloud_message_sealed_body CHECK (sealed IS NULL OR (body='' AND attachments='[]'::jsonb));
ALTER TABLE cloud_node_tasks ADD COLUMN delta_sealed jsonb;
CREATE TABLE cloud_crypto_devices (
 account_id uuid NOT NULL REFERENCES accounts(id), device_id text NOT NULL,
 node_id uuid, binding_version bigint, display_name text,
 revoked boolean NOT NULL DEFAULT false, device_keys jsonb,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(account_id,device_id), CHECK ((node_id IS NULL)=(binding_version IS NULL))
);
CREATE TABLE cloud_crypto_one_time_keys (
 account_id uuid NOT NULL, device_id text NOT NULL, key_id text NOT NULL,
 key_data jsonb NOT NULL, fallback boolean NOT NULL DEFAULT false,
 used boolean NOT NULL DEFAULT false,
 PRIMARY KEY(account_id,device_id,key_id),
 FOREIGN KEY(account_id,device_id) REFERENCES cloud_crypto_devices(account_id,device_id)
);
CREATE TABLE cloud_crypto_signing_keys (
 account_id uuid PRIMARY KEY REFERENCES accounts(id), master_key jsonb,
 self_signing_key jsonb, user_signing_key jsonb
);
CREATE TABLE cloud_crypto_inbox (
 id bigserial PRIMARY KEY, account_id uuid NOT NULL, device_id text NOT NULL,
 event jsonb NOT NULL,
 FOREIGN KEY(account_id,device_id) REFERENCES cloud_crypto_devices(account_id,device_id)
);
CREATE INDEX cloud_crypto_inbox_device ON cloud_crypto_inbox(account_id,device_id,id);
CREATE TABLE cloud_crypto_receipts (
 account_id uuid NOT NULL REFERENCES accounts(id), device_id text NOT NULL,
 request_id text NOT NULL, fingerprint text NOT NULL, response jsonb NOT NULL,
 PRIMARY KEY(account_id,device_id,request_id)
);
CREATE TABLE cloud_crypto_backups (
 account_id uuid NOT NULL REFERENCES accounts(id), version uuid NOT NULL,
 algorithm text NOT NULL, auth_data jsonb NOT NULL, revision bigint NOT NULL DEFAULT 1,
 PRIMARY KEY(account_id,version)
);
CREATE TABLE cloud_crypto_backup_sessions (
 account_id uuid NOT NULL, version uuid NOT NULL, room_id text NOT NULL,
 session_id text NOT NULL, session_data jsonb NOT NULL,
 PRIMARY KEY(account_id,version,room_id,session_id),
 FOREIGN KEY(account_id,version) REFERENCES cloud_crypto_backups(account_id,version)
);
CREATE TABLE cloud_crypto_resources (
 ai_id uuid NOT NULL REFERENCES accounts(id), scope text NOT NULL,
 conversation uuid REFERENCES cloud_conversations(id), item_id text NOT NULL,
 kind text NOT NULL, revision bigint NOT NULL DEFAULT 1, sealed jsonb,
 tombstone text, sources jsonb NOT NULL DEFAULT '[]',
 material_revision uuid NOT NULL, updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(ai_id,scope,item_id),
 CHECK (tombstone IS NULL OR sealed IS NULL)
);
CREATE TABLE cloud_crypto_resource_receipts (
 actor uuid NOT NULL REFERENCES accounts(id), request_id uuid NOT NULL,
 fingerprint text NOT NULL, ai_id uuid NOT NULL, scope text NOT NULL, item_id text NOT NULL,
 PRIMARY KEY(actor,request_id)
);
CREATE TABLE cloud_crypto_applied (
 ai_id uuid NOT NULL REFERENCES accounts(id), scope text NOT NULL, item_id text NOT NULL,
 revision bigint NOT NULL, node_id uuid NOT NULL, binding_version bigint NOT NULL,
 status text NOT NULL, error_code text, updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(ai_id,scope,item_id)
);
CREATE TABLE cloud_crypto_recovery (account_id uuid PRIMARY KEY REFERENCES accounts(id), version text NOT NULL, ciphertext text NOT NULL);
CREATE UNIQUE INDEX cloud_crypto_resource_id ON cloud_crypto_resources(item_id);
CREATE UNIQUE INDEX cloud_crypto_resource_singleton ON cloud_crypto_resources(ai_id,scope) WHERE kind IN ('profile','summarySettings');
ALTER TABLE cloud_crypto_backups ADD COLUMN created_at timestamptz NOT NULL DEFAULT now();
CREATE TABLE cloud_crypto_recovery_history (
 account_id uuid NOT NULL REFERENCES accounts(id), version text NOT NULL, ciphertext text NOT NULL,
 PRIMARY KEY(account_id,version)
);
ALTER TABLE cloud_ai_bindings ADD COLUMN handover_state text NOT NULL DEFAULT 'active' CHECK(handover_state IN ('active','draining','ready'));
