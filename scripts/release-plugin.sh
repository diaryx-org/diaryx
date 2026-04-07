#!/bin/bash
set -e

# Build a release WASM for a plugin and prepare it for CDN upload.
#
# This reads the version from the plugin's Cargo.toml and produces a
# release-ready WASM artifact in dist/plugins/<plugin-id>/.
#
# Usage:
#   ./scripts/release-plugin.sh <plugin-crate-name>
#
# Examples:
#   ./scripts/release-plugin.sh diaryx_sync_extism
#   ./scripts/release-plugin.sh diaryx_publish_extism

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"

if [ -z "$1" ]; then
    echo "Usage: $0 <plugin-crate-name>"
    exit 1
fi

PLUGIN="$1"

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
echo ""
echo "To upload to CDN, use your deployment tooling to push"
echo "  $DIST_DIR/plugin.wasm"
echo "to the plugins CDN path for $PLUGIN v$VERSION."
