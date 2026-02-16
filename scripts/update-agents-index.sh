#!/usr/bin/env bash
# Updates the workspace index in AGENTS.md using diaryx workspace info
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AGENTS_FILE="$REPO_ROOT/AGENTS.md"
TEMP_FILE="$AGENTS_FILE.tmp"
TREE_FILE="$AGENTS_FILE.tree"

# Always clean up temp files, even on error
cleanup() { rm -f "$TEMP_FILE" "$TREE_FILE"; }
trap cleanup EXIT

# Generate full workspace tree
diaryx workspace info --depth 0 "$REPO_ROOT/README.md" --properties title,description,path > "$TREE_FILE"

# Markers: ```workspace-index (opening) and the next ``` (closing)
BEGIN_LINE=$(grep -n '^```workspace-index' "$AGENTS_FILE" | cut -d: -f1)
# Find the closing ``` after the opening marker
END_LINE=$(tail -n +"$((BEGIN_LINE + 1))" "$AGENTS_FILE" | grep -n '^```$' | head -1 | cut -d: -f1)
END_LINE=$((BEGIN_LINE + END_LINE))

# Build new file: head (through opening fence) + tree + closing fence + tail
head -n "$BEGIN_LINE" "$AGENTS_FILE" > "$TEMP_FILE"
cat "$TREE_FILE" >> "$TEMP_FILE"
tail -n +"$END_LINE" "$AGENTS_FILE" >> "$TEMP_FILE"

if cmp -s "$TEMP_FILE" "$AGENTS_FILE"; then
  : # No changes — trap will clean up temp files
else
  mv "$TEMP_FILE" "$AGENTS_FILE"

  # Update frontmatter timestamp
  NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  diaryx property set "$AGENTS_FILE" updated "$NOW"

  echo "Updated AGENTS.md workspace index"
fi
