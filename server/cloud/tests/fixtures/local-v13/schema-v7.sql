CREATE TABLE memories(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 owner_actor_id TEXT NOT NULL REFERENCES ai_profiles(actor_id),
 status TEXT NOT NULL CHECK(status IN ('confirmed','stale','forgotten')),
 revision INTEGER NOT NULL CHECK(revision>0),
 updated_at TEXT NOT NULL,
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id)
);
CREATE INDEX memories_scope ON memories(conversation_id,owner_actor_id,status,updated_at DESC);
CREATE TABLE memory_revisions(
 memory_id TEXT NOT NULL REFERENCES memories(id),
 revision INTEGER NOT NULL CHECK(revision>0),
 payload TEXT NOT NULL CHECK(json_valid(payload)),
 PRIMARY KEY(memory_id,revision)
);
CREATE TABLE memory_message_sources(
 memory_id TEXT NOT NULL REFERENCES memories(id),
 message_id TEXT NOT NULL REFERENCES messages(id),
 revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(memory_id,message_id)
);
CREATE INDEX memories_by_source ON memory_message_sources(message_id);
