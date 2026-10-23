CREATE TABLE cloud_participation (
 conversation uuid PRIMARY KEY REFERENCES cloud_conversations(id), revision bigint NOT NULL DEFAULT 1,
 policy jsonb NOT NULL DEFAULT '{"mode":"mentionedOnly","directMergeMs":800,"maxConsecutiveAi":3,"maxBoutCalls":12,"maxDailyCalls":48,"idleSeconds":300}',
 paused boolean NOT NULL DEFAULT false, paused_ai uuid[] NOT NULL DEFAULT '{}',
 bout_id uuid, consecutive_ai integer NOT NULL DEFAULT 0, bout_calls integer NOT NULL DEFAULT 0,
 last_ai uuid, last_activity timestamptz NOT NULL DEFAULT now(), reason text,
 settings_revision bigint NOT NULL DEFAULT 1, enabled boolean NOT NULL DEFAULT false,
 quiet_until timestamptz, quiet_action text NOT NULL DEFAULT 'defer', budget_day date NOT NULL DEFAULT (now() AT TIME ZONE 'UTC')::date
);
CREATE TABLE cloud_participation_daily(conversation uuid NOT NULL REFERENCES cloud_conversations(id), day date NOT NULL, calls integer NOT NULL DEFAULT 0, PRIMARY KEY(conversation,day));
CREATE TABLE cloud_contact_intents (
 id uuid PRIMARY KEY, conversation uuid NOT NULL REFERENCES cloud_conversations(id), creator_id uuid NOT NULL REFERENCES accounts(id), ai_id uuid NOT NULL REFERENCES accounts(id),
 content text NOT NULL, due_at timestamptz NOT NULL, expires_at timestamptz NOT NULL, utc_offset_minutes integer NOT NULL,
 status text NOT NULL DEFAULT 'pending', revision bigint NOT NULL DEFAULT 1, reason text, task_id uuid,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX cloud_contact_due ON cloud_contact_intents(due_at) WHERE status='pending';
CREATE TABLE cloud_participation_receipts(owner uuid NOT NULL REFERENCES accounts(id),request uuid NOT NULL,fingerprint text NOT NULL, target uuid NOT NULL, PRIMARY KEY(owner,request));
ALTER TABLE cloud_node_tasks ALTER COLUMN input_id DROP NOT NULL;
ALTER TABLE cloud_node_tasks ALTER COLUMN input_revision DROP NOT NULL;
ALTER TABLE cloud_node_tasks ADD COLUMN contact_intent_id uuid UNIQUE REFERENCES cloud_contact_intents(id);
ALTER TABLE cloud_node_tasks ADD COLUMN trigger jsonb;
ALTER TABLE cloud_node_tasks ADD COLUMN input_ids uuid[] NOT NULL DEFAULT '{}';
ALTER TABLE cloud_node_tasks ADD COLUMN automatic boolean NOT NULL DEFAULT false;
ALTER TABLE cloud_node_tasks ADD COLUMN not_before timestamptz NOT NULL DEFAULT now();
ALTER TABLE cloud_node_tasks ADD COLUMN policy_revision bigint;
ALTER TABLE cloud_node_tasks ADD COLUMN ready_notified boolean NOT NULL DEFAULT false;
UPDATE cloud_node_tasks SET input_ids=ARRAY[input_id], trigger=jsonb_build_object('type','message','messageId',input_id,'inputIds',ARRAY[input_id]);
ALTER TABLE cloud_node_tasks ADD CONSTRAINT task_trigger_shape CHECK ((input_id IS NULL) <> (contact_intent_id IS NULL));
