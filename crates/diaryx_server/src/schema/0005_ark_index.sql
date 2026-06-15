-- ARK identity index: maps a (workspace ARK, file ARK) blade pair to the
-- object key it currently resolves to. Populated at publish time — publish is
-- the only door to the server now that sync is gone, so it doubles as
-- registration (provisional file ids become permanent here).
--
-- The file ARK lives in each file's frontmatter, so it is rename-stable; this
-- index is refreshed whenever the file is (re)published. One row per file ARK
-- per workspace: a file rendered to multiple audiences registers only its
-- canonical/primary rendition (multi-audience addressing is a later layer).
--
-- Layer 1 keeps the existing path-based object keying intact; this table is the
-- "alias over paths" stepping stone toward ARK-as-canonical-key.

CREATE TABLE IF NOT EXISTS ark_index (
    workspace_ark TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    file_ark      TEXT NOT NULL,
    object_key    TEXT NOT NULL,
    audience      TEXT,
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (workspace_ark, file_ark)
);

CREATE INDEX IF NOT EXISTS idx_ark_index_ws ON ark_index(workspace_ark);
