CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY CHECK(version>0), checksum TEXT NOT NULL, applied_at TEXT NOT NULL);
CREATE TABLE actors(
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('human','ai')),
 status TEXT NOT NULL CHECK(status IN ('active','archived')), revision INTEGER NOT NULL CHECK(revision>0),
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id)
);
CREATE UNIQUE INDEX single_local_human ON actors(kind) WHERE kind='human';
CREATE TABLE ai_profiles(actor_id TEXT PRIMARY KEY REFERENCES actors(id), revision INTEGER NOT NULL CHECK(revision>0), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TRIGGER profile_ai_only BEFORE INSERT ON ai_profiles WHEN (SELECT kind FROM actors WHERE id=NEW.actor_id)!='ai' BEGIN SELECT RAISE(ABORT,'profile requires AI'); END;
CREATE TABLE execution_bindings(actor_id TEXT PRIMARY KEY REFERENCES ai_profiles(actor_id), revision INTEGER NOT NULL CHECK(revision>0), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE relationships(id TEXT PRIMARY KEY, participant_a TEXT NOT NULL REFERENCES actors(id), participant_b TEXT NOT NULL REFERENCES actors(id), payload TEXT NOT NULL CHECK(json_valid(payload)), CHECK(participant_a<participant_b), UNIQUE(participant_a,participant_b));
CREATE TABLE conversations(
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('direct','group')), relationship_id TEXT UNIQUE REFERENCES relationships(id),
 status TEXT NOT NULL CHECK(status IN ('active','archived')), paused INTEGER NOT NULL CHECK(paused IN (0,1)),
 revision INTEGER NOT NULL CHECK(revision>0), context_epoch INTEGER NOT NULL CHECK(context_epoch>0), participation_epoch INTEGER NOT NULL CHECK(participation_epoch>0), last_sequence INTEGER NOT NULL CHECK(last_sequence>=0),
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id),
 CHECK((kind='direct' AND relationship_id IS NOT NULL) OR (kind='group' AND relationship_id IS NULL))
);
CREATE TABLE memberships(conversation_id TEXT NOT NULL REFERENCES conversations(id), actor_id TEXT NOT NULL REFERENCES actors(id), status TEXT NOT NULL CHECK(status IN ('active','left')), history_from_seq INTEGER NOT NULL CHECK(history_from_seq>=0), revision INTEGER NOT NULL CHECK(revision>0), payload TEXT NOT NULL CHECK(json_valid(payload)), PRIMARY KEY(conversation_id,actor_id));
CREATE TRIGGER direct_members_only BEFORE INSERT ON memberships WHEN (SELECT kind FROM conversations WHERE id=NEW.conversation_id)='direct' AND NOT EXISTS(SELECT 1 FROM relationships r JOIN conversations c ON c.relationship_id=r.id WHERE c.id=NEW.conversation_id AND NEW.actor_id IN (r.participant_a,r.participant_b)) BEGIN SELECT RAISE(ABORT,'direct member is not a relationship participant'); END;
CREATE TABLE bouts(id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id), status TEXT NOT NULL CHECK(status IN ('open','ended')), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE UNIQUE INDEX one_open_bout ON bouts(conversation_id) WHERE status='open';
CREATE TABLE messages(
 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id), sender_id TEXT NOT NULL REFERENCES actors(id),
 sequence INTEGER NOT NULL CHECK(sequence>0), revision INTEGER NOT NULL CHECK(revision>0), status TEXT NOT NULL CHECK(status IN ('committed','generating','interrupted','withdrawn')),
 reply_to TEXT REFERENCES messages(id), run_id TEXT REFERENCES ai_runs(id) DEFERRABLE INITIALLY DEFERRED,
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id), UNIQUE(conversation_id,sequence),
 FOREIGN KEY(conversation_id,sender_id) REFERENCES memberships(conversation_id,actor_id)
);
CREATE TRIGGER reply_same_space BEFORE INSERT ON messages WHEN NEW.reply_to IS NOT NULL AND (SELECT conversation_id FROM messages WHERE id=NEW.reply_to)!=NEW.conversation_id BEGIN SELECT RAISE(ABORT,'reply belongs to another space'); END;
CREATE TABLE message_revisions(message_id TEXT NOT NULL REFERENCES messages(id), revision INTEGER NOT NULL CHECK(revision>0), author_id TEXT NOT NULL REFERENCES actors(id), payload TEXT NOT NULL CHECK(json_valid(payload)), PRIMARY KEY(message_id,revision));
CREATE TABLE read_positions(conversation_id TEXT NOT NULL REFERENCES conversations(id), actor_id TEXT NOT NULL REFERENCES actors(id), sequence INTEGER NOT NULL CHECK(sequence>=0), PRIMARY KEY(conversation_id,actor_id));
CREATE TABLE ai_runs(id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id), message_id TEXT NOT NULL UNIQUE REFERENCES messages(id) DEFERRABLE INITIALLY DEFERRED, bout_id TEXT NOT NULL REFERENCES bouts(id), status TEXT NOT NULL CHECK(status IN ('running','succeeded','failed','cancelled','interrupted')), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE UNIQUE INDEX one_running_ai ON ai_runs(conversation_id) WHERE status='running';
CREATE TABLE committed_events(cursor INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE command_receipts(operator_id TEXT NOT NULL REFERENCES actors(id), kind TEXT NOT NULL, request_id TEXT NOT NULL, digest TEXT NOT NULL, entity_id TEXT NOT NULL, PRIMARY KEY(operator_id,kind,request_id));
CREATE TABLE legacy_imports(source_id TEXT PRIMARY KEY, session_id TEXT NOT NULL UNIQUE, checksum TEXT NOT NULL, conversation_id TEXT NOT NULL UNIQUE REFERENCES conversations(id), ai_actor_id TEXT NOT NULL REFERENCES actors(id), backup_ref TEXT NOT NULL);
CREATE TABLE legacy_session_data(source_id TEXT PRIMARY KEY REFERENCES legacy_imports(source_id), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE legacy_id_map(source_id TEXT NOT NULL REFERENCES legacy_imports(source_id), entry_key TEXT NOT NULL, message_id TEXT NOT NULL UNIQUE, PRIMARY KEY(source_id,entry_key));
CREATE TABLE legacy_entries(source_id TEXT NOT NULL REFERENCES legacy_imports(source_id), entry_key TEXT NOT NULL, message_id TEXT NOT NULL, entry_kind TEXT NOT NULL CHECK(entry_kind IN ('visible','trace','checkpoint')), payload TEXT NOT NULL CHECK(json_valid(payload)), PRIMARY KEY(source_id,entry_key), FOREIGN KEY(source_id,entry_key) REFERENCES legacy_id_map(source_id,entry_key));
