#!/usr/bin/env bash
# Set up macOS dev signing for Diaryx Tauri builds.
#
# Uses your existing Apple Development identity by default (the free one you
# get from Xcode → Settings → Accounts → Apple ID → Manage Certificates). That
# identity is Apple-anchored, so macOS keychain "Always Allow" grants actually
# persist across rebuilds.
#
# To override with a different identity:
#   export DIARYX_DEV_SIGN_IDENTITY="Some Other Identity Name"
#   bash apps/tauri/scripts/setup-macos-dev-signing.sh
#
# The runner (apps/tauri/scripts/macos-dev-sign.sh) reads the same env var, so
# dev builds use whatever you set here.

set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "This script only applies to macOS." >&2
  exit 1
fi

KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
IDENTITY="${DIARYX_DEV_SIGN_IDENTITY:-}"

# Auto-detect first Apple Development identity if none was set
if [ -z "$IDENTITY" ]; then
  IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
    | awk -F'"' '/"Apple Development:/ {print $2; exit}')"
fi

if [ -z "$IDENTITY" ]; then
  cat <<'EOF' >&2
No Apple Development identity found in your keychain.

Options:
  1. Get one (free with any Apple ID):
     Xcode → Settings → Accounts → your Apple ID → Manage Certificates → +
     → "Apple Development"

  2. Use a different identity you already have:
       export DIARYX_DEV_SIGN_IDENTITY="Name as it appears below"
       bash apps/tauri/scripts/setup-macos-dev-signing.sh

Available code-signing identities right now:
EOF
  security find-identity -v -p codesigning >&2 || true
  exit 1
fi

echo "==> Using signing identity: '$IDENTITY'"

# Clean up legacy self-signed 'Diaryx Dev' cert from earlier versions of this script.
if security find-certificate -c "Diaryx Dev" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "==> Removing leftover self-signed 'Diaryx Dev' cert from prior setup..."
  security delete-identity   -c "Diaryx Dev" "$KEYCHAIN" >/dev/null 2>&1 || true
  security delete-certificate -c "Diaryx Dev" "$KEYCHAIN" >/dev/null 2>&1 || true
fi

echo ""
echo "==> Authorizing codesign to use the identity's private key without prompting."
echo "    If prompted, enter your macOS login password:"
if ! security set-key-partition-list \
      -S apple-tool:,apple:,codesign: \
      -s \
      "$KEYCHAIN" >/dev/null 2>&1; then
  echo "NOTE: set-key-partition-list exited non-zero. Usually harmless if Xcode"
  echo "      has already configured the key; codesign will still work."
fi

echo ""
echo "==> Done."
echo ""
echo "Next steps:"
echo "  export DIARYX_DEV_SIGN=1"
if [ -z "${DIARYX_DEV_SIGN_IDENTITY:-}" ]; then
  echo "  # (identity was auto-detected; set DIARYX_DEV_SIGN_IDENTITY to override)"
fi
echo "  bun tauri dev"
echo ""
echo "On the first keychain prompt after signing, click 'Always Allow' once."
echo "Subsequent rebuilds will stay silent as long as the same identity is used."
