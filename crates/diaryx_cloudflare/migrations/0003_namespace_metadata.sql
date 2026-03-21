-- Add free-form JSON metadata column to namespaces.
ALTER TABLE namespaces ADD COLUMN metadata TEXT;
