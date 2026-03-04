#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "Usage: $0 <crate> <target> <out-dir>" >&2
  exit 2
fi

CRATE="$1"
TARGET_NAME="$2"
OUT_DIR="$3"

mkdir -p "$OUT_DIR"

cargo build --target wasm32-unknown-unknown -p "$CRATE" --release

WASM_PATH="target/wasm32-unknown-unknown/release/${TARGET_NAME}.wasm"
if [[ ! -f "$WASM_PATH" ]]; then
  echo "Built WASM not found: $WASM_PATH" >&2
  exit 1
fi

manifest_tmp="$(mktemp)"
trap 'rm -f "$manifest_tmp"' EXIT
cargo run -q -p diaryx_extism --bin inspect_plugin_manifest -- "$WASM_PATH" > "$manifest_tmp"

if command -v sha256sum >/dev/null 2>&1; then
  SHA256="$(sha256sum "$WASM_PATH" | awk '{print tolower($1)}')"
else
  SHA256="$(shasum -a 256 "$WASM_PATH" | awk '{print tolower($1)}')"
fi

SIZE_BYTES="$(wc -c < "$WASM_PATH" | tr -d '[:space:]')"
PUBLISHED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
PLUGIN_ID="$(jq -r '.id' "$manifest_tmp")"

if [[ ! "$PLUGIN_ID" =~ ^diaryx\.[a-z0-9]+([.-][a-z0-9]+)*$ ]]; then
  echo "Plugin ID is not canonical (must be namespaced): $PLUGIN_ID" >&2
  exit 1
fi

ARTIFACT_FILE="${CRATE}.wasm"
cp "$WASM_PATH" "$OUT_DIR/$ARTIFACT_FILE"

jq -n \
  --arg crate "$CRATE" \
  --arg target "$TARGET_NAME" \
  --arg artifactFile "$ARTIFACT_FILE" \
  --arg sha256 "$SHA256" \
  --arg publishedAt "$PUBLISHED_AT" \
  --arg sizeBytes "$SIZE_BYTES" \
  --rawfile manifest "$manifest_tmp" \
  '($manifest | fromjson) as $m
  | {
    crate: $crate,
    target: $target,
    id: $m.id,
    name: $m.name,
    version: $m.version,
    description: $m.description,
    capabilities: ($m.capabilities // []),
    requestedPermissions: ($m.requested_permissions // null),
    artifactFile: $artifactFile,
    sha256: $sha256,
    sizeBytes: ($sizeBytes | tonumber),
    publishedAt: $publishedAt
  }' > "$OUT_DIR/metadata.json"

echo "Built $CRATE -> $OUT_DIR/$ARTIFACT_FILE"
