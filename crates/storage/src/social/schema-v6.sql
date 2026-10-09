CREATE TABLE account_admins(
 ai_id TEXT NOT NULL REFERENCES ai_profiles(actor_id), account_id TEXT NOT NULL REFERENCES actors(id),
 initial INTEGER NOT NULL CHECK(initial IN (0,1)), PRIMARY KEY(ai_id,account_id)
);
CREATE UNIQUE INDEX one_initial_administrator ON account_admins(ai_id) WHERE initial=1;
CREATE TABLE address_book(
 owner_id TEXT NOT NULL REFERENCES actors(id), account_id TEXT NOT NULL REFERENCES actors(id),
 listed INTEGER NOT NULL CHECK(listed IN (0,1)), revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(owner_id,account_id), CHECK(owner_id!=account_id)
);
UPDATE actors SET payload=json_set(payload,'$.bio','');
UPDATE command_receipts SET kind='control_account' WHERE kind='control_contact';
INSERT INTO account_admins SELECT a.id,h.id,1 FROM actors a CROSS JOIN actors h WHERE a.kind='ai' AND h.kind='human';
INSERT INTO address_book SELECT h.id,a.id,CASE WHEN a.status='active' THEN 1 ELSE 0 END,1 FROM actors a CROSS JOIN actors h WHERE a.kind='ai' AND h.kind='human';
INSERT INTO address_book SELECT a.id,h.id,1,1 FROM actors a CROSS JOIN actors h WHERE a.kind='ai' AND h.kind='human';
