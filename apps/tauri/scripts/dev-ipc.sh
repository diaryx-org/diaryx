#!/usr/bin/env bash
# Tiny curl wrapper for the debug-only Tauri IPC listener (see
# src-tauri/src/dev_ipc.rs). Requires the app to be running with
# DIARYX_DEV_IPC=1 (use `bun run tauri:dev-ipc`).
#
# Usage:
#   ./scripts/dev-ipc.sh GET /health
#   ./scripts/dev-ipc.sh GET /state
#   ./scripts/dev-ipc.sh GET "/log?tail=100"
#   ./scripts/dev-ipc.sh POST /execute --data '{"type":"GetEntry","params":{"path":"README.md"}}'
#   ./scripts/dev-ipc.sh POST /emit    --data '{"event":"x","payload":{}}'
#   ./scripts/dev-ipc.sh POST /eval    --data '{"js":"alert(1)"}'   # needs DIARYX_DEV_IPC_EVAL=1
#   ./scripts/dev-ipc.sh GET  /screenshot > /tmp/diaryx.png   # binary PNG
#   ./scripts/dev-ipc.sh GET  "/screenshot?format=json"       # base64-wrapped
#
# Prints only the response body to stdout (binary-safe — no trailing
# newline is appended for non-text responses). On non-2xx, the body
# still prints (usually a JSON {"error": ...}) and the script exits
# with the mapped status code so callers can branch on success.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
DISC="$HERE/.dev-ipc.json"

if [ ! -f "$DISC" ]; then
  echo "error: no discovery file at $DISC" >&2
  echo "       start the app with: bun run tauri:dev-ipc" >&2
  exit 2
fi

read -r PORT TOKEN < <(python3 -c "
import json,sys
d=json.load(open(sys.argv[1]))
print(d['port'], d['token'])
" "$DISC")

METHOD="${1:-GET}"
ENDPOINT="${2:-/health}"
shift 2 2>/dev/null || true

TMP="$(mktemp -t diaryx-dev-ipc.XXXXXX)"
trap 'rm -f "$TMP"' EXIT

META=$(curl -sS -o "$TMP" -w '%{http_code}\n%{content_type}' \
  -X "$METHOD" \
  -H "X-Diaryx-Dev-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  "$@" \
  "http://127.0.0.1:$PORT$ENDPOINT")

STATUS=$(printf '%s\n' "$META" | sed -n '1p')
CTYPE=$(printf '%s\n' "$META" | sed -n '2p')

cat "$TMP"
# Add a trailing newline only for JSON/text so terminals read cleanly;
# never for binary (would corrupt PNG/etc).
case "$CTYPE" in
  application/json*|text/*|'')
    [ -s "$TMP" ] && [ "$(tail -c1 "$TMP" | xxd -p)" != "0a" ] && echo
    ;;
esac

case "$STATUS" in
  2*) exit 0 ;;
  4*) exit 4 ;;
  5*) exit 5 ;;
  *)  exit 1 ;;
esac
