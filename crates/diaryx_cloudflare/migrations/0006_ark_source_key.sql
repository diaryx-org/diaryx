-- Layer 2: record the markdown *source* object a file ARK resolves to, next to
-- its rendered HTML (`object_key`). Populated at publish when the client
-- uploads the source sibling and sends `X-Diaryx-Source-Key`. Nullable: rows
-- registered before Layer 2 (HTML-only) simply have no source to resolve, so
-- `?content`/`?json`/`?info` return 404 for them.

ALTER TABLE ark_index ADD COLUMN source_key TEXT;
