#!/bin/bash
set -euo pipefail

# Build the diaryx_apple Rust crate and generate UniFFI Swift bindings.
#
# Output:
#   apps/apple/diaryx_apple.xcframework/   — static XCFramework for Xcode
#   apps/apple/Diaryx/Generated/           — generated Swift bindings
#
# Usage:
#   ./build-rust.sh              # Release build, all platforms
#   ./build-rust.sh debug        # Debug build, all platforms
#   ./build-rust.sh release mac  # Release build, macOS only

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Ensure rustup-managed toolchain is visible (needed under Nix)
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.local/share/rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.local/share/cargo}"

# Resolve Xcode SDK paths directly from the filesystem (xcrun fails under Nix)
XCODE_DEV="/Applications/Xcode.app/Contents/Developer"
IPHONEOS_SDK="$XCODE_DEV/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
IPHONESIM_SDK="$XCODE_DEV/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk"
MACOS_SDK="$XCODE_DEV/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
XCODE_CLANG="$XCODE_DEV/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang"

# iOS device
export CC_aarch64_apple_ios="$XCODE_CLANG"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$XCODE_CLANG"
export CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS="-C link-arg=-isysroot -C link-arg=$IPHONEOS_SDK"
export SDKROOT_IPHONEOS="$IPHONEOS_SDK"

# iOS simulator
export CC_aarch64_apple_ios_sim="$XCODE_CLANG"
export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER="$XCODE_CLANG"
export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUSTFLAGS="-C link-arg=-isysroot -C link-arg=$IPHONESIM_SDK"

# macOS (in case Nix interferes here too)
export SDKROOT="$MACOS_SDK"

PROFILE="${1:-release}"
PLATFORM="${2:-all}"
PROFILE_DIR="$PROFILE"
CARGO_FLAGS=(--release)
if [ "$PROFILE" = "debug" ]; then
    CARGO_FLAGS=()
    PROFILE_DIR="debug"
fi

LIB_NAME="libdiaryx_apple.a"
TARGET_DIR="$REPO_ROOT/target"

GENERATED_DIR="$SCRIPT_DIR/Diaryx/Generated"
XCFRAMEWORK_DIR="$SCRIPT_DIR/diaryx_apple.xcframework"
HEADERS_DIR="$SCRIPT_DIR/headers"

# --- Build targets ---

MACOS_TARGET="aarch64-apple-darwin"
IOS_TARGET="aarch64-apple-ios"
IOS_SIM_TARGET="aarch64-apple-ios-sim"

build_target() {
    local target="$1"
    echo "==> Building diaryx_apple ($PROFILE, $target)..."
    cargo build -p diaryx_apple --target "$target" "${CARGO_FLAGS[@]}"
}

# Always build macOS
build_target "$MACOS_TARGET"

if [ "$PLATFORM" = "all" ]; then
    build_target "$IOS_TARGET"
    build_target "$IOS_SIM_TARGET"
fi

# --- Generate UniFFI bindings (using the macOS library) ---

MACOS_LIB="$TARGET_DIR/$MACOS_TARGET/$PROFILE_DIR/$LIB_NAME"

echo "==> Generating UniFFI Swift bindings..."
cargo run -p diaryx_apple --features bindgen --bin uniffi-bindgen -- \
    generate --library "$MACOS_LIB" --language swift --out-dir "$SCRIPT_DIR/uniffi-out"

# --- Assemble XCFramework ---

echo "==> Assembling XCFramework..."
rm -rf "$XCFRAMEWORK_DIR" "$HEADERS_DIR"
mkdir -p "$HEADERS_DIR"

cp "$SCRIPT_DIR/uniffi-out/diaryx_appleFFI.h" "$HEADERS_DIR/"
cp "$SCRIPT_DIR/uniffi-out/diaryx_appleFFI.modulemap" "$HEADERS_DIR/module.modulemap"

if [ "$PLATFORM" = "all" ]; then
    IOS_LIB="$TARGET_DIR/$IOS_TARGET/$PROFILE_DIR/$LIB_NAME"
    IOS_SIM_LIB="$TARGET_DIR/$IOS_SIM_TARGET/$PROFILE_DIR/$LIB_NAME"

    xcodebuild -create-xcframework \
        -library "$MACOS_LIB" \
        -headers "$HEADERS_DIR" \
        -library "$IOS_LIB" \
        -headers "$HEADERS_DIR" \
        -library "$IOS_SIM_LIB" \
        -headers "$HEADERS_DIR" \
        -output "$XCFRAMEWORK_DIR"
else
    xcodebuild -create-xcframework \
        -library "$MACOS_LIB" \
        -headers "$HEADERS_DIR" \
        -output "$XCFRAMEWORK_DIR"
fi

# --- Copy generated Swift bindings ---

echo "==> Copying generated Swift bindings..."
mkdir -p "$GENERATED_DIR"
cp "$SCRIPT_DIR/uniffi-out/diaryx_apple.swift" "$GENERATED_DIR/diaryx_apple.swift"

# Cleanup temp dirs
rm -rf "$SCRIPT_DIR/uniffi-out" "$HEADERS_DIR"

echo "==> Done. Artifacts:"
echo "    $XCFRAMEWORK_DIR"
echo "    $GENERATED_DIR/diaryx_apple.swift"
