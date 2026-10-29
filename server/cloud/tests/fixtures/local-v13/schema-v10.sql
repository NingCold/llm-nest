CREATE TABLE participation_policies (
 conversation_id TEXT PRIMARY KEY REFERENCES conversations(id),
 payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE participation_state (
 conversation_id TEXT PRIMARY KEY REFERENCES conversations(id),
 payload TEXT NOT NULL CHECK(json_valid(payload))
);
-- Debits are never refunded by retries, failures, new human input or ending a Bout.
CREATE TABLE participation_daily_budget (
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 day TEXT NOT NULL,
 turns INTEGER NOT NULL,
 tokens INTEGER NOT NULL,
 PRIMARY KEY(conversation_id,day)
);
