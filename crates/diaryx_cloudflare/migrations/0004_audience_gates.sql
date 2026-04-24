-- Replaces `namespace_audiences.access` with a stackable `gates` JSON column.
-- Matches crates/diaryx_server/src/schema/0004_audience_gates.sql.

ALTER TABLE namespace_audiences ADD COLUMN gates TEXT NOT NULL DEFAULT '[]';

UPDATE namespace_audiences
   SET gates = '[{"kind":"link"}]'
 WHERE access = 'token';

UPDATE namespace_audiences
   SET gates = '[]'
 WHERE access = 'public';

DELETE FROM namespace_audiences WHERE access = 'private';
