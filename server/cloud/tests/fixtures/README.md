# Local payload compatibility probe

`local-human-payloads.json` uses the `social::Actor` and `social::SocialMessage`
JSON payload format stored by local SQLite (`crates/storage/src/social`). It is
synthetic data, not a user database or a migration package. The associated
PostgreSQL test deserializes those real shared types and checks a test-only field
mapping; no production import endpoint is added.

The probe preserves human/account, conversation and message UUIDs, name, public
bio, original text, sender identity, timestamps, message revision, original image
and file bytes, and relative message order. Local per-window sequence numbers
are ordered into cloud delivery positions rather than copied into the global
position counter. Original text part indices differ from cloud attachment-only
indices; a future importer must record the deterministic part-index mapping.

The cloud UUID columns can retain stable ActorId values directly. This active
human private-chat sample does not settle migration for archived-account state,
avatars, account revision, historical sender-name snapshots, interleaved content
parts, AI identity/private material, groups, replies or context references. Those
need explicit mapping in the planned backup/preview migration work; they are not
silently imported by this probe.
