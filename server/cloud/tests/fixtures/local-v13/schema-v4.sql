-- All current members share the complete history of their conversation.
ALTER TABLE memberships DROP COLUMN history_from_seq;
UPDATE memberships SET payload=json_remove(payload,'$.history_from_seq');
