-- Reference payloads live with their private messages. These indexes track all
-- original dependencies, including the originals covered by a selected summary.
CREATE TABLE context_references (
    message_id TEXT PRIMARY KEY REFERENCES messages(id),
    source_conversation_id TEXT NOT NULL REFERENCES conversations(id),
    actor_id TEXT NOT NULL REFERENCES actors(id)
);
CREATE INDEX references_by_group ON context_references(source_conversation_id,actor_id);
CREATE TABLE context_reference_sources (
    reference_message_id TEXT NOT NULL REFERENCES context_references(message_id),
    source_message_id TEXT NOT NULL REFERENCES messages(id),
    PRIMARY KEY(reference_message_id,source_message_id)
);
CREATE INDEX references_by_source ON context_reference_sources(source_message_id);
