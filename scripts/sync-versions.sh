#!/usr/bin/env bash
# Syncs version from README.md frontmatter to all project files
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Extract version from README.md frontmatter using diaryx CLI (e.g., "v0.9.0" -> "0.9.0")
VERSION=$(diaryx property get "$REPO_ROOT/README.md" version | sed 's/^v//')

if [[ -z "$VERSION" ]]; then
    echo "Error: Could not find version in README.md frontmatter"
    exit 1
fi

echo "Syncing version: $VERSION"

update_flake_package_version() {
    local pname="$1"
    perl -0pi.bak -e 's/(pname = "'"$pname"'";\n\s+version = ")[^"]+(")/${1}'"$VERSION"'${2}/' "$REPO_ROOT/flake.nix"
}

# Update root Cargo.toml workspace version
sed -i.bak -E 's/^(version = ")[^"]+(")/\1'"$VERSION"'\2/' "$REPO_ROOT/Cargo.toml"
rm -f "$REPO_ROOT/Cargo.toml.bak"

# Update diaryx_core dependency version in root Cargo.toml
sed -i.bak -E 's/(diaryx_core = \{ path = "[^"]+", version = ")[^"]+(" \})/\1'"$VERSION"'\2/' "$REPO_ROOT/Cargo.toml"
rm -f "$REPO_ROOT/Cargo.toml.bak"

# Update diaryx_sync dependency version in root Cargo.toml
sed -i.bak -E 's/(diaryx_sync = \{ path = "[^"]+", version = ")[^"]+(" \})/\1'"$VERSION"'\2/' "$REPO_ROOT/Cargo.toml"
rm -f "$REPO_ROOT/Cargo.toml.bak"

# Update tauri.conf.json
sed -i.bak -E 's/("version": ")[^"]+(")/\1'"$VERSION"'\2/' "$REPO_ROOT/apps/tauri/src-tauri/tauri.conf.json"
rm -f "$REPO_ROOT/apps/tauri/src-tauri/tauri.conf.json.bak"

# Update apps/web/package.json
sed -i.bak -E 's/("version": ")[^"]+(")/\1'"$VERSION"'\2/' "$REPO_ROOT/apps/web/package.json"
rm -f "$REPO_ROOT/apps/web/package.json.bak"

# Update flake.nix package versions without touching dependency/tool versions.
update_flake_package_version "diaryx"
update_flake_package_version "diaryx-sync-server"
update_flake_package_version "ts-bindings"
update_flake_package_version "wasm-package"
rm -f "$REPO_ROOT/flake.nix.bak"

echo "Version synced to $VERSION in all files"
