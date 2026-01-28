#!/bin/bash
set -e

# Get the directory of the script
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"

echo "Building WASM in $WORKSPACE_ROOT/crates/diaryx_wasm..."

cd "$WORKSPACE_ROOT/crates/diaryx_wasm"
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
