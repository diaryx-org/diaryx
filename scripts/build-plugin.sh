#!/bin/bash
set -e

# Build a Diaryx plugin WASM binary.
#
# Usage:
#   ./scripts/build-plugin.sh <plugin-crate-name> [--release]
#
# Examples:
#   ./scripts/build-plugin.sh diaryx_sync_extism --release
#   ./scripts/build-plugin.sh diaryx_publish_extism

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"

if [ -z "$1" ]; then
    echo "Usage: $0 <plugin-crate-name> [--release]"
    echo ""
    echo "Available plugins:"
    for dir in "$WORKSPACE_ROOT"/crates/plugins/*/; do
        name=$(basename "$dir")
        # Skip the SDK — it's a library, not a WASM plugin
        if [ "$name" = "diaryx_plugin_sdk" ]; then continue; fi
        echo "  $name"
    done
    exit 1
fi

PLUGIN="$1"
shift

PROFILE="debug"
CARGO_FLAGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --release) PROFILE="release"; CARGO_FLAGS+=(--release); shift ;;
        *) CARGO_FLAGS+=("$1"); shift ;;
    esac
done

cd "$WORKSPACE_ROOT"

echo "Building $PLUGIN (target: wasm32-unknown-unknown, profile: $PROFILE)"
cargo build --target wasm32-unknown-unknown -p "$PLUGIN" "${CARGO_FLAGS[@]}"

WASM_FILE="target/wasm32-unknown-unknown/$PROFILE/${PLUGIN}.wasm"

if [ ! -f "$WASM_FILE" ]; then
    echo "Error: expected output not found at $WASM_FILE"
    exit 1
fi

SIZE=$(wc -c < "$WASM_FILE" | tr -d ' ')
echo "Built: $WASM_FILE ($SIZE bytes)"

# Optimize with wasm-opt if available and in release mode
if [ "$PROFILE" = "release" ] && command -v wasm-opt >/dev/null 2>&1; then
    echo "Running wasm-opt -Oz..."
    wasm-opt -Oz -o "$WASM_FILE" "$WASM_FILE"
    OPT_SIZE=$(wc -c < "$WASM_FILE" | tr -d ' ')
    echo "Optimized: $WASM_FILE ($OPT_SIZE bytes)"
fi
