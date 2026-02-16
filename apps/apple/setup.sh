#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> Building editor bundle..."
cd editor-bundle
if ! command -v bun &> /dev/null; then
    echo "Error: bun is required. Install from https://bun.sh"
    exit 1
fi
bun install
bun run build
cd ..

echo "==> Building Rust library and generating UniFFI bindings..."
./build-rust.sh

echo "==> Generating Xcode project..."
if ! command -v xcodegen &> /dev/null; then
    echo "xcodegen not found. Installing via Homebrew..."
    if ! command -v brew &> /dev/null; then
        echo "Error: Homebrew is required to install xcodegen."
        echo "Install from https://brew.sh, then run this script again."
        echo "Or install xcodegen manually: https://github.com/yonaskolb/XcodeGen"
        exit 1
    fi
    brew install xcodegen
fi

xcodegen generate

echo ""
echo "Done! Open Diaryx.xcodeproj in Xcode:"
echo "  open Diaryx.xcodeproj"
