CREATE TABLE cloud_direct_grants (
 token_hash text PRIMARY KEY, human_id uuid NOT NULL REFERENCES accounts(id),
 ai_id uuid NOT NULL REFERENCES accounts(id), conversation uuid NOT NULL REFERENCES cloud_conversations(id),
 node_id uuid NOT NULL, binding_version bigint NOT NULL
);
CREATE TABLE cloud_direct_receipts (
 ai_id uuid NOT NULL REFERENCES accounts(id), request_id uuid NOT NULL,
 fingerprint text NOT NULL, message_ids uuid[] NOT NULL, PRIMARY KEY(ai_id,request_id)
);
CREATE TABLE cloud_direct_runs (
 run_id uuid PRIMARY KEY, message_id uuid NOT NULL UNIQUE REFERENCES cloud_messages(id),
 ai_id uuid NOT NULL REFERENCES accounts(id), binding_version bigint NOT NULL,
 configuration_revision text NOT NULL, usage jsonb, error_code text
);
