#!/usr/bin/env bash
# Cargo `runner` for Diaryx Tauri dev builds on macOS.
#
# Invoked by cargo as: macos-dev-sign.sh <binary> [args...]
#
# When DIARYX_DEV_SIGN=1 is set, re-signs the debug binary with a stable
# self-signed identity (default "Diaryx Dev") before executing it. A stable
# signature means the macOS keychain's "Always Allow" grant persists across
# rebuilds, so `bun tauri dev` stops prompting for keychain access every run.
#
# Otherwise, just execs the binary unchanged.
#
# Setup: run scripts/setup-macos-dev-signing.sh once to create the identity.

set -euo pipefail

if [ $# -lt 1 ]; then
  cat >&2 <<EOF
$(basename "$0") is a cargo \`runner\` — not meant to be invoked directly.
Cargo calls it as: $(basename "$0") <binary> [args...]

To sign and run dev builds, use:
  DIARYX_DEV_SIGN=1 bun tauri dev

See apps/tauri/README.md (macOS dev signing section) and
scripts/setup-macos-dev-signing.sh for setup.
EOF
  exit 64
fi

BIN="$1"
shift

if [ "${DIARYX_DEV_SIGN:-}" = "1" ] && [ "$(uname)" = "Darwin" ]; then
  IDENTITY="${DIARYX_DEV_SIGN_IDENTITY:-}"
  # Auto-detect first Apple Development identity if the caller didn't set one.
  if [ -z "$IDENTITY" ]; then
    IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
      | awk -F'"' '/"Apple Development:/ {print $2; exit}')"
  fi
  if [ -n "$IDENTITY" ]; then
    if ! codesign --force --sign "$IDENTITY" "$BIN" 2>/tmp/diaryx-dev-sign.err; then
      echo "WARN: codesign with identity '$IDENTITY' failed:" >&2
      cat /tmp/diaryx-dev-sign.err >&2
      echo "      Continuing with existing (ad-hoc) signature." >&2
    fi
  else
    echo "WARN: DIARYX_DEV_SIGN=1 but no signing identity found." >&2
    echo "      Set DIARYX_DEV_SIGN_IDENTITY or run scripts/setup-macos-dev-signing.sh." >&2
  fi
fi

exec "$BIN" "$@"
