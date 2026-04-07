#!/bin/bash
set -e

# Build a release WASM for a plugin and prepare it for CDN upload.
#
# This reads the version from the plugin's Cargo.toml and produces a
# release-ready WASM artifact in dist/plugins/<plugin-id>/.
#
# With --upload, also creates a GitHub Release on diaryx-org/diaryx and
# opens a PR against the plugin-registry repo to update the registry entry.
# Requires `gh` CLI authenticated with push access to plugin-registry.
#
# Usage:
#   ./scripts/release-plugin.sh <plugin-crate-name> [--upload]
#
# Examples:
#   ./scripts/release-plugin.sh diaryx_sync_extism
#   ./scripts/release-plugin.sh diaryx_publish_extism --upload

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"

UPLOAD=false

# Parse arguments
PLUGIN=""
for arg in "$@"; do
    case "$arg" in
        --upload) UPLOAD=true ;;
        -*) echo "Unknown flag: $arg"; exit 1 ;;
        *) PLUGIN="$arg" ;;
    esac
done

if [ -z "$PLUGIN" ]; then
    echo "Usage: $0 <plugin-crate-name> [--upload]"
    exit 1
fi

# Resolve crate directory
CRATE_DIR="$WORKSPACE_ROOT/crates/plugins/$PLUGIN"
if [ ! -d "$CRATE_DIR" ]; then
    echo "Error: crate directory not found at $CRATE_DIR"
    exit 1
fi

# Extract version from Cargo.toml
# Handles both version = "x.y.z" and version.workspace = true
VERSION=$(grep '^version' "$CRATE_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
if [ -z "$VERSION" ] || echo "$VERSION" | grep -q 'workspace'; then
    # Workspace version — read from root
    VERSION=$(grep '^version' "$WORKSPACE_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
fi

echo "=== Releasing $PLUGIN v$VERSION ==="

# Build release WASM
"$SCRIPT_DIR/build-plugin.sh" "$PLUGIN" --release

WASM_SRC="$WORKSPACE_ROOT/target/wasm32-unknown-unknown/release/${PLUGIN}.wasm"

# Prepare output directory
DIST_DIR="$WORKSPACE_ROOT/dist/plugins/$PLUGIN"
mkdir -p "$DIST_DIR"

cp "$WASM_SRC" "$DIST_DIR/plugin.wasm"

# Generate SHA256 checksum
if command -v sha256sum >/dev/null 2>&1; then
    SHA=$(sha256sum "$DIST_DIR/plugin.wasm" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
    SHA=$(shasum -a 256 "$DIST_DIR/plugin.wasm" | cut -d' ' -f1)
else
    echo "Warning: no sha256sum or shasum found, skipping checksum"
    SHA="unknown"
fi

SIZE=$(wc -c < "$DIST_DIR/plugin.wasm" | tr -d ' ')

echo ""
echo "=== Release artifact ready ==="
echo "  Path:    $DIST_DIR/plugin.wasm"
echo "  Version: $VERSION"
echo "  Size:    $SIZE bytes"
echo "  SHA256:  $SHA"

# Derive plugin ID from crate name: diaryx_sync_extism -> diaryx.sync
PLUGIN_ID=$(echo "$PLUGIN" | sed 's/_extism$//' | sed 's/_/./')

if [ "$UPLOAD" = true ]; then
    echo ""
    echo "=== Uploading $PLUGIN_ID v$VERSION ==="

    # --- GitHub Release ---
    TAG="$PLUGIN_ID/v$VERSION"
    RELEASE_NAME="$PLUGIN_ID v$VERSION"

    echo "Creating GitHub Release: $TAG"
    gh release create "$TAG" \
        "$DIST_DIR/plugin.wasm#${PLUGIN}.wasm" \
        --repo diaryx-org/diaryx \
        --title "$RELEASE_NAME" \
        --generate-notes \
        --notes-start-tag "$(gh release list --repo diaryx-org/diaryx --json tagName -q "[.[] | select(.tagName | startswith(\"$PLUGIN_ID/\"))][1].tagName // \"\"" 2>/dev/null)"

    DOWNLOAD_URL="https://github.com/diaryx-org/diaryx/releases/download/${TAG}/${PLUGIN}.wasm"
    NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

    echo "Release created: $DOWNLOAD_URL"

    # --- Update plugin-registry ---
    echo "Updating plugin-registry..."

    REGISTRY_DIR=$(mktemp -d)
    trap 'rm -rf "$REGISTRY_DIR"' EXIT

    gh repo clone diaryx-org/plugin-registry "$REGISTRY_DIR" -- --depth 1
    PLUGIN_FILE="$REGISTRY_DIR/plugins/${PLUGIN_ID}.md"
    BRANCH="release/${PLUGIN_ID}-v${VERSION}"

    cd "$REGISTRY_DIR"
    git config user.name "$(git -C "$WORKSPACE_ROOT" config user.name)"
    git config user.email "$(git -C "$WORKSPACE_ROOT" config user.email)"

    NEW_PLUGIN=false
    if [ ! -f "$PLUGIN_FILE" ]; then
        NEW_PLUGIN=true
        TITLE=$(echo "$PLUGIN_ID" | awk -F. '{print $NF}' | sed 's/.*/\u&/')
        mkdir -p "$(dirname "$PLUGIN_FILE")"
        cat > "$PLUGIN_FILE" <<HEREDOC
---
title: "${TITLE}"
description: ""
id: "${PLUGIN_ID}"
version: "${VERSION}"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: []
tags: []
capabilities: []
artifact:
  url: "${DOWNLOAD_URL}"
  sha256: "${SHA}"
  size: ${SIZE}
  published_at: "${NOW}"
---

TODO: add description
HEREDOC
    else
        python3 -c "
import re, sys
text = open('$PLUGIN_FILE').read()
text = re.sub(r'version: \".*?\"', 'version: \"$VERSION\"', text, count=1)
text = re.sub(r'url: \".*?\"', 'url: \"$DOWNLOAD_URL\"', text, count=1)
text = re.sub(r'sha256: \".*?\"', 'sha256: \"$SHA\"', text, count=1)
text = re.sub(r'size: \d+', 'size: $SIZE', text, count=1)
text = re.sub(r'published_at: \".*?\"', 'published_at: \"$NOW\"', text, count=1)
open('$PLUGIN_FILE', 'w').write(text)
"
    fi

    git checkout -b "$BRANCH"
    git add "$PLUGIN_FILE"
    git commit -m "release: ${PLUGIN_ID} v${VERSION}"
    git push origin "$BRANCH"

    PR_BODY="Automated release from [diaryx ${TAG}](https://github.com/diaryx-org/diaryx/releases/tag/${TAG})"
    if [ "$NEW_PLUGIN" = true ]; then
        PR_BODY="${PR_BODY}

> **New plugin** — description, categories, tags, capabilities, and UI slots need to be filled in before merging."
    fi

    PR_URL=$(gh pr create \
        --repo diaryx-org/plugin-registry \
        --title "release: ${PLUGIN_ID} v${VERSION}" \
        --body "$PR_BODY" \
        --head "$BRANCH")

    cd "$WORKSPACE_ROOT"

    echo ""
    echo "=== Upload complete ==="
    echo "  GitHub Release: https://github.com/diaryx-org/diaryx/releases/tag/${TAG}"
    echo "  Registry PR:    $PR_URL"
else
    echo ""
    echo "To upload, re-run with --upload:"
    echo "  $0 $PLUGIN --upload"
fi
