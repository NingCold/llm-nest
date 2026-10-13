CREATE TABLE contact_settings(
 conversation_id TEXT PRIMARY KEY REFERENCES conversations(id),
 payload TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE TABLE contact_intents(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 actor_id TEXT NOT NULL REFERENCES ai_profiles(actor_id),
 created_by TEXT NOT NULL REFERENCES actors(id),
 status TEXT NOT NULL CHECK(status IN ('pending','claimed','executing','delivered','failed','cancelled','expired','skipped','uncertain')),
 due_at TEXT NOT NULL,
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id),
 FOREIGN KEY(conversation_id,actor_id) REFERENCES memberships(conversation_id,actor_id)
);
CREATE INDEX due_contacts ON contact_intents(conversation_id,due_at) WHERE status IN ('pending','claimed');

DROP TRIGGER plan_same_space;
DROP INDEX pending_participation;
DROP INDEX one_running_plan;
ALTER TABLE participation_plans RENAME TO participation_plans_v10;
CREATE TABLE participation_plans(
 id TEXT PRIMARY KEY,
 conversation_id TEXT NOT NULL REFERENCES conversations(id),
 actor_id TEXT NOT NULL REFERENCES ai_profiles(actor_id),
 trigger_id TEXT REFERENCES messages(id),
 trigger_sequence INTEGER NOT NULL CHECK(trigger_sequence>=0),
 ordinal INTEGER NOT NULL CHECK(ordinal>=0),
 status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled','interrupted')),
 run_id TEXT UNIQUE REFERENCES ai_runs(id),
 payload TEXT NOT NULL CHECK(json_valid(payload) AND json_extract(payload,'$.id')=id),
 contact_intent_id TEXT UNIQUE REFERENCES contact_intents(id),
 CHECK((trigger_id IS NULL) != (contact_intent_id IS NULL)),
 UNIQUE(trigger_id,actor_id),
 FOREIGN KEY(conversation_id,actor_id) REFERENCES memberships(conversation_id,actor_id)
);
INSERT INTO participation_plans SELECT *,NULL FROM participation_plans_v10;
DROP TABLE participation_plans_v10;
CREATE INDEX pending_participation ON participation_plans(conversation_id,trigger_sequence,ordinal) WHERE status IN ('queued','running');
CREATE UNIQUE INDEX message_plan_order ON participation_plans(conversation_id,trigger_sequence,ordinal) WHERE trigger_id IS NOT NULL;
CREATE UNIQUE INDEX one_running_plan ON participation_plans(conversation_id) WHERE status='running';
CREATE TRIGGER plan_same_space BEFORE INSERT ON participation_plans
 WHEN (NEW.trigger_id IS NOT NULL AND (SELECT conversation_id FROM messages WHERE id=NEW.trigger_id)!=NEW.conversation_id)
   OR (NEW.contact_intent_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM contact_intents WHERE id=NEW.contact_intent_id AND conversation_id=NEW.conversation_id AND actor_id=NEW.actor_id))
 BEGIN SELECT RAISE(ABORT,'plan trigger belongs to another space'); END;
