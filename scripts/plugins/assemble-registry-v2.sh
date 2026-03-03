#!/usr/bin/env bash
set -euo pipefail

BUILD_DIR="${1:-plugin-packages}"
OUTPUT_PATH="${2:-plugin-registry/registry-v2.json}"
INTERNAL_CATALOG="${3:-plugins/catalog/internal-plugins.json}"
EXTERNAL_CATALOG="${4:-plugins/catalog/external-plugins.json}"
CDN_BASE_URL="${5:-https://cdn.diaryx.org}"
UPLOAD_PLAN_PATH="${6:-plugin-registry/upload-plan.json}"

mkdir -p "$(dirname "$OUTPUT_PATH")"
mkdir -p "$(dirname "$UPLOAD_PLAN_PATH")"

mapfile -t metadata_files < <(find "$BUILD_DIR" -type f -name metadata.json | sort)
if [[ ${#metadata_files[@]} -eq 0 ]]; then
  echo "No plugin metadata files found in: $BUILD_DIR" >&2
  exit 1
fi

built_tmp="$(mktemp)"
versions_tmp="$(mktemp)"
trap 'rm -f "$built_tmp" "$versions_tmp"' EXIT

jq -s '.' "${metadata_files[@]}" > "$built_tmp"

cargo metadata --no-deps --format-version 1 | jq '
  .packages
  | map({ key: .name, value: .version })
  | from_entries
' > "$versions_tmp"

GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

jq -n \
  --arg generatedAt "$GENERATED_AT" \
  --arg cdnBaseUrl "$CDN_BASE_URL" \
  --argfile internal "$INTERNAL_CATALOG" \
  --argfile external "$EXTERNAL_CATALOG" \
  --argfile built "$built_tmp" \
  --argfile versions "$versions_tmp" \
'
  def assert_or_error($cond; $msg): if $cond then . else error($msg) end;

  def validate_internal_plugin($p; $catalog; $versions; $cdn):
    ($catalog.plugins | map({ key: .id, value: . }) | from_entries) as $catalog_by_id
    | ($catalog_by_id[$p.id] // error("Missing internal catalog entry for plugin id: \($p.id)")) as $c
    | .
    | assert_or_error(($p.id | test("^diaryx\\.[a-z0-9]+([.-][a-z0-9]+)*$")); "Invalid canonical plugin id: \($p.id)")
    | assert_or_error(($c.crate == $p.crate); "Catalog crate mismatch for \($p.id): expected \($c.crate), got \($p.crate)")
    | assert_or_error(($c.name == $p.name); "Catalog name mismatch for \($p.id): expected \($c.name), got \($p.name)")
    | assert_or_error(($c.description == $p.description); "Catalog description mismatch for \($p.id)")
    | assert_or_error((($versions[$p.crate] // "") == $p.version); "Cargo version mismatch for \($p.id): expected \($versions[$p.crate]), got \($p.version)")
    | {
        id: $p.id,
        name: $p.name,
        version: $p.version,
        summary: $c.summary,
        description: $c.description,
        creator: ($catalog.creator // "Diaryx Team"),
        license: ($catalog.license // "PolyForm Shield 1.0.0"),
        source: {
          kind: ($catalog.source.kind // "internal"),
          repositoryUrl: ($catalog.source.repositoryUrl // "https://github.com/diaryx-org/diaryx"),
          registryId: ($catalog.registryId // "diaryx-official")
        },
        artifact: {
          wasmUrl: ($cdn + "/plugins/artifacts/" + $p.id + "/" + $p.version + "/" + $p.sha256 + ".wasm"),
          sha256: $p.sha256,
          sizeBytes: $p.sizeBytes,
          publishedAt: $p.publishedAt
        },
        homepage: ($c.homepage // null),
        documentationUrl: ($c.documentationUrl // null),
        changelogUrl: ($c.changelogUrl // null),
        categories: ($c.categories // []),
        tags: ($c.tags // []),
        iconUrl: ($c.iconUrl // null),
        screenshots: ($c.screenshots // []),
        capabilities: ($p.capabilities // []),
        requestedPermissions: ($p.requestedPermissions // null)
      };

  def validate_external_plugin($p):
    .
    | assert_or_error(($p.id | test("^diaryx\\.[a-z0-9]+([.-][a-z0-9]+)*$")); "Invalid external plugin id: \($p.id)")
    | assert_or_error((($p.source.kind // "") == "external"); "External plugin must use source.kind='external': \($p.id)")
    | assert_or_error(((($p.artifact.sha256 // "") | length) > 0); "External plugin missing artifact.sha256: \($p.id)")
    | assert_or_error(((($p.artifact.wasmUrl // "") | length) > 0); "External plugin missing artifact.wasmUrl: \($p.id)")
    | assert_or_error(((($p.version // "") | length) > 0); "External plugin missing version: \($p.id)")
    | $p
    | .categories = (.categories // [])
    | .tags = (.tags // [])
    | .screenshots = (.screenshots // [])
    | .requestedPermissions = (.requestedPermissions // null)
    | .iconUrl = (.iconUrl // null)
    | .homepage = (.homepage // null)
    | .documentationUrl = (.documentationUrl // null)
    | .changelogUrl = (.changelogUrl // null);

  ($built | map(validate_internal_plugin(.; $internal; $versions; $cdnBaseUrl))) as $internal_plugins
  | ($external.plugins // [] | map(validate_external_plugin(.))) as $external_plugins
  | {
      schemaVersion: 2,
      generatedAt: $generatedAt,
      plugins: (($internal_plugins + $external_plugins) | sort_by(.id))
    }
' > "$OUTPUT_PATH"

jq -n \
  --arg cdnBaseUrl "$CDN_BASE_URL" \
  --argfile built "$built_tmp" \
'
  $built
  | map({
      id,
      version,
      sha256,
      artifactFile,
      s3Key: ("plugins/artifacts/" + .id + "/" + .version + "/" + .sha256 + ".wasm"),
      wasmUrl: ($cdnBaseUrl + "/plugins/artifacts/" + .id + "/" + .version + "/" + .sha256 + ".wasm")
    })
' > "$UPLOAD_PLAN_PATH"

jq -e '.schemaVersion == 2' "$OUTPUT_PATH" >/dev/null

echo "Wrote registry: $OUTPUT_PATH"
echo "Wrote upload plan: $UPLOAD_PLAN_PATH"
