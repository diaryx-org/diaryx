#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="$REPO_ROOT/scripts/.env.publish"

if [ ! -f "$ENV_FILE" ]; then
  echo "ERROR: $ENV_FILE not found. Copy .env.publish.example and fill in your values."
  exit 1
fi
source "$ENV_FILE"

# ── Paths ────────────────────────────────────────────────────────────
TAURI_DIR="$REPO_ROOT/apps/tauri"
SRC_TAURI="$TAURI_DIR/src-tauri"
APP_BUNDLE="$REPO_ROOT/target/release/bundle/macos/Diaryx.app"
ENTITLEMENTS="$SRC_TAURI/Entitlements.plist"
PROVISIONING_PROFILE="$SRC_TAURI/embedded.provisionprofile"
PKG_OUTPUT="$REPO_ROOT/Diaryx.pkg"

# ── Build number ─────────────────────────────────────────────────────
BUILD_NUMBER="${1:-}"
if [ -z "$BUILD_NUMBER" ]; then
  echo "Usage: $0 <build-number>"
  echo "  e.g. $0 2"
  echo ""
  echo "The build number must be higher than the last uploaded build."
  echo "The marketing version comes from tauri.conf.json."
  exit 1
fi

# ── Step 1: Build ────────────────────────────────────────────────────
echo "==> Building Diaryx.app..."
cd "$TAURI_DIR"
cargo tauri build --bundles app -- --features apple

# ── Step 1b: Fix Nix dylib paths ─────────────────────────────────────
BINARY="$APP_BUNDLE/Contents/MacOS/diaryx_tauri"
echo "==> Rewriting Nix dylib paths to system libraries..."
otool -L "$BINARY" | { grep '/nix/store\|/Volumes/VOLUME' || true; } | awk '{print $1}' | while read -r nix_path; do
  lib_name="$(basename "$nix_path")"
  system_path="/usr/lib/$lib_name"
  echo "    $nix_path -> $system_path"
  install_name_tool -change "$nix_path" "$system_path" "$BINARY"
done

# ── Step 2: Set build number ─────────────────────────────────────────
echo "==> Setting CFBundleVersion to $BUILD_NUMBER..."
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUMBER" \
  "$APP_BUNDLE/Contents/Info.plist"

# ── Step 3: Embed provisioning profile ─────────────────────────────
echo "==> Embedding provisioning profile..."
if [ ! -f "$PROVISIONING_PROFILE" ]; then
  echo "ERROR: $PROVISIONING_PROFILE not found."
  echo "Download it from https://developer.apple.com/account/resources/profiles/list"
  exit 1
fi
cp "$PROVISIONING_PROFILE" "$APP_BUNDLE/Contents/embedded.provisionprofile"
xattr -cr "$APP_BUNDLE"

# ── Step 4: Build entitlements with application identifier ─────────
ENTITLEMENTS_RESOLVED="$(mktemp)"
trap 'rm -f "$ENTITLEMENTS_RESOLVED"' EXIT
sed "s|</dict>|    <key>com.apple.application-identifier</key>\n    <string>${APPLE_TEAM_ID}.org.diaryx.desktop</string>\n</dict>|" \
  "$ENTITLEMENTS" > "$ENTITLEMENTS_RESOLVED"

# ── Step 5: Sign the .app ───────────────────────────────────────────
echo "==> Signing Diaryx.app..."
codesign --deep --force --options runtime \
  --sign "$APP_SIGN_IDENTITY" \
  --entitlements "$ENTITLEMENTS_RESOLVED" \
  "$APP_BUNDLE"

# ── Step 6: Package as .pkg ──────────────────────────────────────────
echo "==> Creating Diaryx.pkg..."
productbuild \
  --component "$APP_BUNDLE" /Applications \
  --sign "$PKG_SIGN_IDENTITY" \
  "$PKG_OUTPUT"

# ── Step 7: Upload ───────────────────────────────────────────────────
echo "==> Uploading to App Store Connect..."
xcrun altool --upload-app --type macos \
  --file "$PKG_OUTPUT" \
  --apiKey "$API_KEY" \
  --apiIssuer "$API_ISSUER"

echo "==> Done! Check App Store Connect for processing status."
