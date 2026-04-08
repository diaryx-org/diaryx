#!/usr/bin/env bash
# Build the HEIC converter plugin as a WASM module.
#
# Prerequisites:
#   - wasi-sdk installed (set WASI_SDK_PATH or default /opt/wasi-sdk)
#   - rustup target add wasm32-wasip1
#   - wasm-opt (in PATH or via flake.nix)
set -euo pipefail

cd "$(dirname "$0")"

cargo build --target wasm32-wasip1 --release

WASM_PATH="target/wasm32-wasip1/release/diaryx_heic_extism.wasm"
if command -v wasm-opt &>/dev/null; then
  wasm-opt -Oz "$WASM_PATH" -o "$WASM_PATH.opt"
  mv "$WASM_PATH.opt" "$WASM_PATH"
  echo "Optimized with wasm-opt: $(wc -c < "$WASM_PATH") bytes"
else
  echo "wasm-opt not found, skipping optimization"
  echo "Size: $(wc -c < "$WASM_PATH") bytes"
fi

echo "Plugin built: $WASM_PATH"
