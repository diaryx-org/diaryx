#!/bin/bash
set -euo pipefail

# Build the diaryx_apple Rust crate and generate UniFFI Swift bindings.
#
# Output:
#   apps/apple/diaryx_apple.xcframework/   — static XCFramework for Xcode
#   apps/apple/Diaryx/Generated/           — generated Swift bindings

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TARGET="aarch64-apple-darwin"
PROFILE="${1:-release}"
PROFILE_DIR="$PROFILE"
CARGO_FLAGS=(--release)
if [ "$PROFILE" = "debug" ]; then
    CARGO_FLAGS=()
    PROFILE_DIR="debug"
fi

LIB_NAME="libdiaryx_apple.a"
TARGET_DIR="$REPO_ROOT/target"
LIB_PATH="$TARGET_DIR/$TARGET/$PROFILE_DIR/$LIB_NAME"

GENERATED_DIR="$SCRIPT_DIR/Diaryx/Generated"
XCFRAMEWORK_DIR="$SCRIPT_DIR/diaryx_apple.xcframework"
HEADERS_DIR="$SCRIPT_DIR/headers"

echo "==> Building diaryx_apple ($PROFILE, $TARGET)..."
cargo build -p diaryx_apple --target "$TARGET" "${CARGO_FLAGS[@]}"

echo "==> Generating UniFFI Swift bindings..."
cargo run -p diaryx_apple --features bindgen --bin uniffi-bindgen -- \
    generate --library "$LIB_PATH" --language swift --out-dir "$SCRIPT_DIR/uniffi-out"

echo "==> Assembling XCFramework..."
rm -rf "$XCFRAMEWORK_DIR" "$HEADERS_DIR"
mkdir -p "$HEADERS_DIR"

# Copy header and modulemap into a headers directory for xcodebuild
cp "$SCRIPT_DIR/uniffi-out/diaryx_appleFFI.h" "$HEADERS_DIR/"
cp "$SCRIPT_DIR/uniffi-out/diaryx_appleFFI.modulemap" "$HEADERS_DIR/module.modulemap"

xcodebuild -create-xcframework \
    -library "$LIB_PATH" \
    -headers "$HEADERS_DIR" \
    -output "$XCFRAMEWORK_DIR"

echo "==> Copying generated Swift bindings..."
mkdir -p "$GENERATED_DIR"
cp "$SCRIPT_DIR/uniffi-out/diaryx_apple.swift" "$GENERATED_DIR/diaryx_apple.swift"

# Cleanup temp dirs
rm -rf "$SCRIPT_DIR/uniffi-out" "$HEADERS_DIR"

echo "==> Done. Artifacts:"
echo "    $XCFRAMEWORK_DIR"
echo "    $GENERATED_DIR/diaryx_apple.swift"
