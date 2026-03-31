#!/bin/bash
set -e

# Get the directory of the script
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"
WEB_DIR="$WORKSPACE_ROOT/apps/web"

echo "Building WASM from workspace: $WORKSPACE_ROOT"

# Verify wasm-pack is available on PATH (installed via cargo or nix)
if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "Error: wasm-pack not found. Install it with: cargo install wasm-pack"
    exit 1
fi

echo "Using wasm-pack at: $(command -v wasm-pack)"
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

        # Force host build scripts and proc-macro crates to use the Xcode toolchain
        # instead of any ambient Nix cc-wrapper state.
        export CC=/usr/bin/cc
        export CXX=/usr/bin/c++
        export AR=/usr/bin/ar
        export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/cc
    fi
fi

# Change to workspace root first to ensure cargo can find workspace
cd "$WORKSPACE_ROOT"
# Build with wasm-pack, then run wasm-opt with aggressive size optimization.
# wasm-pack runs wasm-opt internally but only with default flags; re-running
# with -Oz squeezes out additional bytes.
wasm-pack build crates/diaryx_wasm --target web --out-dir "$WEB_DIR/src/lib/wasm"

WASM_FILE="$WEB_DIR/src/lib/wasm/diaryx_wasm_bg.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
    echo "Running wasm-opt -Oz on $WASM_FILE"
    wasm-opt -Oz -o "$WASM_FILE" "$WASM_FILE"
else
    echo "wasm-opt not found, skipping additional size optimization"
fi

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
