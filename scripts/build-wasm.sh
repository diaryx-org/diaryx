#!/bin/bash
set -e

# Get the directory of the script
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"
WEB_DIR="$WORKSPACE_ROOT/apps/web"

echo "Building WASM from workspace: $WORKSPACE_ROOT"

# Check if node_modules exists and has wasm-pack, install dependencies if not
if [ ! -f "$WEB_DIR/node_modules/.bin/wasm-pack" ]; then
    echo "wasm-pack not found, installing dependencies..."
    cd "$WEB_DIR"
    bun install
    cd "$WORKSPACE_ROOT"
fi

# Verify wasm-pack is available
if [ ! -f "$WEB_DIR/node_modules/.bin/wasm-pack" ]; then
    echo "Error: wasm-pack still not found after installing dependencies"
    exit 1
fi

echo "Using wasm-pack at: $WEB_DIR/node_modules/.bin/wasm-pack"
echo "Building in directory: $WORKSPACE_ROOT/crates/diaryx_wasm"

# Change to workspace root first to ensure cargo can find workspace
cd "$WORKSPACE_ROOT"
$WEB_DIR/node_modules/.bin/wasm-pack build crates/diaryx_wasm --target web --out-dir "$WEB_DIR/src/lib/wasm"
