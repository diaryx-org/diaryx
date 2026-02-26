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

# ── Build and export ─────────────────────────────────────────────────
echo "==> Building iOS app..."
cd "$TAURI_DIR"

APPLE_API_KEY="$API_KEY" \
APPLE_API_ISSUER="$API_ISSUER" \
APPLE_API_KEY_PATH="$API_KEY_PATH" \
cargo tauri ios build --export-method app-store-connect -- --features iap

# ── Find the IPA ─────────────────────────────────────────────────────
IPA=$(find "$REPO_ROOT/apps/tauri/src-tauri/gen/apple/build" -name "*.ipa" -newer "$0" | head -1)

if [ -z "$IPA" ]; then
  echo "ERROR: Could not find .ipa file. Check the build output above."
  exit 1
fi

echo "==> Found IPA: $IPA"

# ── Upload ───────────────────────────────────────────────────────────
echo "==> Uploading to App Store Connect..."
xcrun altool --upload-app --type ios \
  --file "$IPA" \
  --apiKey "$API_KEY" \
  --apiIssuer "$API_ISSUER"

echo "==> Done! Check App Store Connect for processing status."
