ALTER TABLE cloud_messages ADD COLUMN crypto_audience_revision bigint;
ALTER TABLE cloud_contact_intents ADD COLUMN sealed jsonb, ADD COLUMN crypto_audience_revision bigint;
ALTER TABLE cloud_messages DROP CONSTRAINT cloud_message_sealed_body;
ALTER TABLE cloud_messages ADD CONSTRAINT cloud_message_sealed_body CHECK (
 sealed IS NULL OR (body='' AND NOT jsonb_path_exists(attachments, '$[*] ? (@.kind != "encrypted")'))
);
ALTER TABLE cloud_contact_intents ADD CONSTRAINT cloud_contact_sealed_body CHECK (sealed IS NULL OR content='');
