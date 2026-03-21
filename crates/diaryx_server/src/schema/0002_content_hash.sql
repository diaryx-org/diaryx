-- Add content_hash column for content-addressed deduplication.
ALTER TABLE namespace_objects ADD COLUMN content_hash TEXT;

-- Index for ref-counting blob keys within a namespace.
CREATE INDEX IF NOT EXISTS idx_namespace_objects_r2_key ON namespace_objects(namespace_id, r2_key);
