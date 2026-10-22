CREATE TABLE cloud_ai_admins (
    ai_id uuid NOT NULL REFERENCES accounts(id),
    administrator uuid NOT NULL REFERENCES accounts(id),
    initial boolean NOT NULL DEFAULT false,
    PRIMARY KEY (ai_id, administrator)
);
CREATE UNIQUE INDEX cloud_ai_initial_admin ON cloud_ai_admins(ai_id) WHERE initial;
CREATE TABLE cloud_node_pairings (
    id uuid PRIMARY KEY,
    node_id uuid NOT NULL,
    node_name text NOT NULL,
    token_hash text NOT NULL UNIQUE,
    user_code text NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    ai_id uuid REFERENCES accounts(id),
    binding_version bigint
);
CREATE TABLE cloud_ai_bindings (
    ai_id uuid PRIMARY KEY REFERENCES accounts(id),
    version bigint NOT NULL DEFAULT 0,
    node_id uuid,
    node_name text,
    token_hash text UNIQUE,
    last_seen_at timestamptz,
    connection_id uuid
);
CREATE TABLE cloud_node_tasks (
    id uuid PRIMARY KEY,
    ai_id uuid NOT NULL REFERENCES accounts(id),
    conversation uuid NOT NULL REFERENCES cloud_conversations(id),
    input_id uuid NOT NULL REFERENCES cloud_messages(id),
    input_revision bigint NOT NULL,
    binding_version bigint NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    state text NOT NULL DEFAULT 'queued',
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    sources jsonb NOT NULL DEFAULT '[]',
    start_request uuid,
    start_fingerprint text,
    snapshot_id text,
    run_id uuid UNIQUE,
    message_id uuid UNIQUE,
    confirmation_id uuid UNIQUE,
    started_at timestamptz,
    result_request uuid,
    result_fingerprint text,
    result jsonb,
    finished_at timestamptz,
    error_code text,
    delta_sequence bigint NOT NULL DEFAULT 0,
    delta_text text NOT NULL DEFAULT '',
    UNIQUE (ai_id, input_id)
);
CREATE INDEX cloud_node_tasks_pending ON cloud_node_tasks(ai_id, created_at) WHERE state IN ('queued', 'running');
ALTER TABLE cloud_messages ADD COLUMN status text NOT NULL DEFAULT 'committed';
ALTER TABLE cloud_messages ADD COLUMN ai_response jsonb;
CREATE TABLE cloud_node_events (
    id bigserial PRIMARY KEY,
    ai_id uuid NOT NULL REFERENCES accounts(id),
    binding_version bigint NOT NULL,
    task_id uuid REFERENCES cloud_node_tasks(id),
    kind text NOT NULL
);
CREATE INDEX cloud_node_events_ai ON cloud_node_events(ai_id, id);
CREATE TABLE cloud_ai_receipts (
    owner uuid NOT NULL REFERENCES accounts(id),
    request uuid NOT NULL,
    fingerprint text NOT NULL,
    response jsonb NOT NULL,
    PRIMARY KEY (owner, request)
);
