CREATE TABLE participation_plans(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 actor_id TEXT NOT NULL REFERENCES ai_profiles(actor_id),
 trigger_id TEXT NOT NULL REFERENCES messages(id),
 trigger_sequence INTEGER NOT NULL CHECK(trigger_sequence>0),
 ordinal INTEGER NOT NULL CHECK(ordinal>=0),
 status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled','interrupted')),
 run_id TEXT UNIQUE REFERENCES ai_runs(id),
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id),
 UNIQUE(trigger_id,actor_id), UNIQUE(conversation_id,trigger_sequence,ordinal),
 FOREIGN KEY(conversation_id,actor_id) REFERENCES memberships(conversation_id,actor_id)
);
CREATE INDEX pending_participation ON participation_plans(conversation_id,trigger_sequence,ordinal) WHERE status IN ('queued','running');
CREATE UNIQUE INDEX one_running_plan ON participation_plans(conversation_id) WHERE status='running';
CREATE TRIGGER plan_same_space BEFORE INSERT ON participation_plans
 WHEN (SELECT conversation_id FROM messages WHERE id=NEW.trigger_id)!=NEW.conversation_id
 BEGIN SELECT RAISE(ABORT,'plan trigger belongs to another space'); END;
