CREATE TABLE summary_settings (
    id TEXT PRIMARY KEY REFERENCES conversations(id),
    payload TEXT NOT NULL
);
CREATE TABLE conversation_summaries (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    from_sequence INTEGER NOT NULL,
    through_sequence INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','running','ready','failed','interrupted','stale')),
    revision INTEGER NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX summaries_window ON conversation_summaries(conversation_id, through_sequence);
CREATE UNIQUE INDEX summaries_one_task ON conversation_summaries(conversation_id) WHERE status IN ('queued','running');
CREATE TABLE summary_revisions (
    summary_id TEXT NOT NULL REFERENCES conversation_summaries(id),
    revision INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(summary_id, revision)
);
CREATE TABLE summary_message_sources (
    summary_id TEXT NOT NULL REFERENCES conversation_summaries(id),
    message_id TEXT NOT NULL REFERENCES messages(id),
    revision INTEGER NOT NULL,
    PRIMARY KEY(summary_id, message_id)
);
CREATE INDEX summary_sources_message ON summary_message_sources(message_id);
