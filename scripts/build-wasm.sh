#!/bin/bash
set -e

# Get the directory of the script
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"
WEB_DIR="$WORKSPACE_ROOT/apps/web"

echo "Building WASM in $WORKSPACE_ROOT/crates/diaryx_wasm..."

# Check if node_modules exists and has wasm-pack, install dependencies if not
if [ ! -f "$WEB_DIR/node_modules/.bin/wasm-pack" ]; then
    echo "wasm-pack not found, installing dependencies..."
    cd "$WEB_DIR"
    bun install
    cd "$WORKSPACE_ROOT"
fi

cd "$WORKSPACE_ROOT/crates/diaryx_wasm"
../../apps/web/node_modules/.bin/wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
