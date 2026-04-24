-- Replaces `namespace_audiences.access` with a stackable `gates` JSON column.
--
-- A gate record is a JSON object tagged by `kind`:
--   {"kind": "link"}
--   {"kind": "password", "hash": <argon2-string-or-null>, "version": <u32>}
--
-- An empty JSON array `[]` means the audience is public. Multiple gates on the
-- same audience are evaluated with OR semantics by the site proxy.
--
-- Backfill mapping:
--   access = 'public'  → gates = '[]'
--   access = 'token'   → gates = '[{"kind":"link"}]'
--   access = 'private' → row is deleted (legacy dead state)
--
-- The `access` column is left in place for this migration so in-flight code
-- paths keep compiling; it is ignored by new code and scheduled for removal
-- in a follow-up migration.

ALTER TABLE namespace_audiences ADD COLUMN gates TEXT NOT NULL DEFAULT '[]';

UPDATE namespace_audiences
   SET gates = '[{"kind":"link"}]'
 WHERE access = 'token';

UPDATE namespace_audiences
   SET gates = '[]'
 WHERE access = 'public';

DELETE FROM namespace_audiences WHERE access = 'private';
