-- Delivery order differs from message order: a streaming reply can finish late.
CREATE TABLE message_deliveries(
  position INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id TEXT NOT NULL UNIQUE REFERENCES messages(id),
  conversation_id TEXT NOT NULL REFERENCES conversations(id)
);
CREATE INDEX deliveries_conversation ON message_deliveries(conversation_id,position);
INSERT INTO message_deliveries(message_id,conversation_id)
SELECT id,conversation_id FROM messages WHERE status IN ('committed','interrupted') ORDER BY rowid;
CREATE TRIGGER message_delivered_insert AFTER INSERT ON messages
WHEN NEW.status IN ('committed','interrupted') BEGIN
  INSERT INTO message_deliveries(message_id,conversation_id) VALUES(NEW.id,NEW.conversation_id);
END;
CREATE TRIGGER message_delivered_update AFTER UPDATE OF status ON messages
WHEN OLD.status='generating' AND NEW.status IN ('committed','interrupted') BEGIN
  INSERT INTO message_deliveries(message_id,conversation_id) VALUES(NEW.id,NEW.conversation_id);
END;
CREATE TABLE conversation_list_state(
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  actor_id TEXT NOT NULL REFERENCES actors(id),
  read_through INTEGER NOT NULL DEFAULT 0 CHECK(read_through>=0),
  marked_unread INTEGER NOT NULL DEFAULT 0 CHECK(marked_unread IN (0,1)),
  pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)),
  revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
  PRIMARY KEY(conversation_id,actor_id)
);
-- Existing history starts read; future completed replies are new arrivals.
INSERT INTO conversation_list_state(conversation_id,actor_id,read_through)
SELECT m.conversation_id,m.actor_id,COALESCE((SELECT MAX(position) FROM message_deliveries d WHERE d.conversation_id=m.conversation_id),0) FROM memberships m;
DROP TABLE read_positions;
