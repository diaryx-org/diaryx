#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 5 ]]; then
  echo "Usage: $0 <build-dir> <upload-plan.json> <registry.json> <r2-endpoint> <r2-bucket>" >&2
  exit 2
fi

BUILD_DIR="$1"
UPLOAD_PLAN="$2"
REGISTRY_JSON="$3"
R2_ENDPOINT="$4"
R2_BUCKET="$5"

if [[ ! -f "$UPLOAD_PLAN" ]]; then
  echo "Upload plan not found: $UPLOAD_PLAN" >&2
  exit 1
fi
if [[ ! -f "$REGISTRY_JSON" ]]; then
  echo "Registry file not found: $REGISTRY_JSON" >&2
  exit 1
fi

jq -c '.[]' "$UPLOAD_PLAN" | while IFS= read -r item; do
  artifact_file="$(jq -r '.artifactFile' <<< "$item")"
  s3_key="$(jq -r '.s3Key' <<< "$item")"
  wasm_path="$(find "$BUILD_DIR" -type f -name "$artifact_file" | head -n1)"

  if [[ -z "$wasm_path" || ! -f "$wasm_path" ]]; then
    echo "WASM artifact not found for $artifact_file" >&2
    exit 1
  fi

  echo "Uploading $wasm_path -> s3://$R2_BUCKET/$s3_key"
  aws s3 cp "$wasm_path" "s3://$R2_BUCKET/$s3_key" \
    --endpoint-url "$R2_ENDPOINT" \
    --content-type "application/wasm" \
    --cache-control "public, max-age=31536000, immutable"
done

echo "Uploading registry -> s3://$R2_BUCKET/plugins/registry-v2.json"
aws s3 cp "$REGISTRY_JSON" "s3://$R2_BUCKET/plugins/registry-v2.json" \
  --endpoint-url "$R2_ENDPOINT" \
  --content-type "application/json" \
  --cache-control "public, max-age=300"
