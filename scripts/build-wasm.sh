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

# On macOS, propagate Xcode SDK settings so host build helpers don't emit
# repeated `xcrun --sdk macosx --show-sdk-path` warnings.
if [[ "$OSTYPE" == darwin* ]]; then
    if command -v xcode-select >/dev/null 2>&1 && command -v xcrun >/dev/null 2>&1; then
        export DEVELOPER_DIR="${DEVELOPER_DIR:-$(xcode-select -p 2>/dev/null || true)}"
        if [ -z "${SDKROOT:-}" ]; then
            # Prefer the explicit macOS SDK identifier rustc probes for.
            SDK_PATH="$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)"
            if [ -z "$SDK_PATH" ]; then
                SDK_PATH="$(xcrun --show-sdk-path 2>/dev/null || true)"
            fi
            export SDKROOT="$SDK_PATH"
        fi
    fi
fi

# Change to workspace root first to ensure cargo can find workspace
cd "$WORKSPACE_ROOT"
$WEB_DIR/node_modules/.bin/wasm-pack build crates/diaryx_wasm --target web --out-dir "$WEB_DIR/src/lib/wasm"
