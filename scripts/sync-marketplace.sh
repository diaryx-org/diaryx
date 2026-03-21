#!/usr/bin/env bash
# Fetch marketplace registries from the production CDN into apps/web/marketplace-dist.
#
# Run once after cloning, or whenever registries are updated upstream:
#   ./scripts/sync-marketplace.sh
#
# WASM artifacts are NOT downloaded — they're fetched at install time from the CDN.
# Only lightweight registry files (.md) and bundle/template metadata (.json) are synced.

set -euo pipefail

CDN_ORIGIN="${CDN_ORIGIN:-https://app.diaryx.org}"
DIST_DIR="$(cd "$(dirname "$0")/../apps/web/marketplace-dist" && pwd)"

registries=(
  "plugins/registry.md"
  "bundles/registry.md"
  "themes/registry.md"
  "typographies/registry.md"
  "templates/registry.md"
  "starter-workspaces/registry.md"
)

echo "Syncing marketplace registries from $CDN_ORIGIN/cdn → $DIST_DIR"

for reg in "${registries[@]}"; do
  url="$CDN_ORIGIN/cdn/$reg"
  dest="$DIST_DIR/$reg"
  mkdir -p "$(dirname "$dest")"

  printf "  %-40s " "$reg"
  status=$(curl -sS -o "$dest" -w "%{http_code}" "$url" 2>/dev/null) || true
  if [ "$status" = "200" ]; then
    echo "OK"
  else
    echo "SKIP ($status)"
    rm -f "$dest"
  fi
done

echo "Done. Run 'bun run dev' from apps/web to serve locally."
