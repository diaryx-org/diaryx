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

# Build the Extism sync plugin WASM
echo "Building diaryx_sync_extism WASM plugin..."
SYNC_PLUGIN_DIR="$WEB_DIR/public/plugins"
mkdir -p "$SYNC_PLUGIN_DIR"
cargo build --target wasm32-unknown-unknown -p diaryx_sync_extism --release
cp "$WORKSPACE_ROOT/target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm" \
   "$SYNC_PLUGIN_DIR/diaryx_sync.wasm"
# Optimize with wasm-opt if available
if command -v wasm-opt >/dev/null 2>&1; then
    echo "Optimizing sync plugin WASM with wasm-opt..."
    wasm-opt -Oz "$SYNC_PLUGIN_DIR/diaryx_sync.wasm" -o "$SYNC_PLUGIN_DIR/diaryx_sync.wasm"
fi

# Guardrail: Extism guest plugins must not import wasm-bindgen placeholders.
if LC_ALL=C grep -a -q "__wbindgen_placeholder__" "$SYNC_PLUGIN_DIR/diaryx_sync.wasm"; then
    echo "Error: sync plugin contains wasm-bindgen imports (__wbindgen_placeholder__)."
    echo "Expected an Extism guest artifact from crates/diaryx_sync_extism."
    exit 1
fi

echo "Sync plugin WASM built: $SYNC_PLUGIN_DIR/diaryx_sync.wasm"

# Build the AI chat plugin WASM
echo "Building diaryx_ai_extism WASM plugin..."
cargo build --target wasm32-unknown-unknown -p diaryx_ai_extism --release
cp "$WORKSPACE_ROOT/target/wasm32-unknown-unknown/release/diaryx_ai_extism.wasm" \
   "$SYNC_PLUGIN_DIR/diaryx_ai.wasm"
# Optimize with wasm-opt if available
if command -v wasm-opt >/dev/null 2>&1; then
    echo "Optimizing AI plugin WASM with wasm-opt..."
    wasm-opt -Oz "$SYNC_PLUGIN_DIR/diaryx_ai.wasm" -o "$SYNC_PLUGIN_DIR/diaryx_ai.wasm"
fi

# Guardrail: AI plugin must not import wasm-bindgen placeholders.
if LC_ALL=C grep -a -q "__wbindgen_placeholder__" "$SYNC_PLUGIN_DIR/diaryx_ai.wasm"; then
    echo "Error: AI plugin contains wasm-bindgen imports (__wbindgen_placeholder__)."
    exit 1
fi

echo "AI plugin WASM built: $SYNC_PLUGIN_DIR/diaryx_ai.wasm"

# Build the Publish plugin WASM
echo "Building diaryx_publish_extism WASM plugin..."
cargo build --target wasm32-unknown-unknown -p diaryx_publish_extism --release
cp "$WORKSPACE_ROOT/target/wasm32-unknown-unknown/release/diaryx_publish_extism.wasm" \
   "$SYNC_PLUGIN_DIR/diaryx_publish.wasm"
# Optimize with wasm-opt if available
if command -v wasm-opt >/dev/null 2>&1; then
    echo "Optimizing Publish plugin WASM with wasm-opt..."
    wasm-opt -Oz "$SYNC_PLUGIN_DIR/diaryx_publish.wasm" -o "$SYNC_PLUGIN_DIR/diaryx_publish.wasm"
fi

# Guardrail: Publish plugin must not import wasm-bindgen placeholders.
if LC_ALL=C grep -a -q "__wbindgen_placeholder__" "$SYNC_PLUGIN_DIR/diaryx_publish.wasm"; then
    echo "Error: Publish plugin contains wasm-bindgen imports (__wbindgen_placeholder__)."
    exit 1
fi

echo "Publish plugin WASM built: $SYNC_PLUGIN_DIR/diaryx_publish.wasm"

# Build the Math plugin WASM
echo "Building diaryx_math_extism WASM plugin..."
cargo build --target wasm32-unknown-unknown -p diaryx_math_extism --release
cp "$WORKSPACE_ROOT/target/wasm32-unknown-unknown/release/diaryx_math_extism.wasm" \
   "$SYNC_PLUGIN_DIR/diaryx_math.wasm"
# Optimize with wasm-opt if available
if command -v wasm-opt >/dev/null 2>&1; then
    echo "Optimizing Math plugin WASM with wasm-opt..."
    wasm-opt -Oz "$SYNC_PLUGIN_DIR/diaryx_math.wasm" -o "$SYNC_PLUGIN_DIR/diaryx_math.wasm"
fi

# Guardrail: Math plugin must not import wasm-bindgen placeholders.
if LC_ALL=C grep -a -q "__wbindgen_placeholder__" "$SYNC_PLUGIN_DIR/diaryx_math.wasm"; then
    echo "Error: Math plugin contains wasm-bindgen imports (__wbindgen_placeholder__)."
    exit 1
fi

echo "Math plugin WASM built: $SYNC_PLUGIN_DIR/diaryx_math.wasm"

# Clean up trailing whitespace in ts-rs generated bindings
# (ts-rs emits trailing spaces on struct field lines)
BINDINGS_DIR="$WORKSPACE_ROOT/crates/diaryx_core/bindings"
if [ -d "$BINDINGS_DIR" ]; then
    # Portable in-place sed: GNU sed uses -i, BSD sed uses -i ''
    if sed --version >/dev/null 2>&1; then
        SED_INPLACE=(sed -i)
    else
        SED_INPLACE=(sed -i '')
    fi
    find "$BINDINGS_DIR" -name '*.ts' -exec "${SED_INPLACE[@]}" 's/[[:space:]]*$//' {} +
    echo "Cleaned trailing whitespace in ts-rs bindings"
fi
