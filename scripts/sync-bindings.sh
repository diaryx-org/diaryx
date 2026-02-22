#!/bin/bash
set -euo pipefail

# Syncs ts-rs generated TypeScript bindings into the web app's generated/ directory
# as symlinks, and auto-generates the barrel index.ts.
#
# Source tree:
#   crates/diaryx_core/bindings/
#     bindings/*.ts            → diaryx types
#     serde_json/JsonValue.ts  → external crate types
#
# Destination:
#   apps/web/src/lib/backend/generated/
#     *.ts          → symlinks to source
#     serde_json/   → symlinks to source
#     index.ts      → auto-generated barrel
#
# Usage: ./scripts/sync-bindings.sh

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_ROOT="$( dirname "$SCRIPT_DIR" )"

BINDINGS_ROOT="$WORKSPACE_ROOT/crates/diaryx_core/bindings"
GENERATED_DIR="$WORKSPACE_ROOT/apps/web/src/lib/backend/generated"

if [ ! -d "$BINDINGS_ROOT" ]; then
    echo "Error: bindings directory not found at $BINDINGS_ROOT"
    echo "Run 'cargo test -p diaryx_core' to generate bindings first."
    exit 1
fi

mkdir -p "$GENERATED_DIR"

# ── Helper: create or update a symlink ─────────────────────────────────────────

link_file() {
    local src="$1" dest="$2"
    local target
    target="$(python3 -c "import os.path; print(os.path.relpath('$src', '$(dirname "$dest")'))")"

    if [ -L "$dest" ] && [ "$(readlink "$dest")" = "$target" ]; then
        return  # already correct
    fi

    rm -f "$dest"
    ln -s "$target" "$dest"
    echo "  linked: $(basename "$dest")"
}

# ── Step 1: Clean generated/ ──────────────────────────────────────────────────
# Remove all .ts files except index.ts (regular files = stale copies,
# broken symlinks = removed source files).

for f in "$GENERATED_DIR"/*.ts; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    [ "$(basename "$f")" = "index.ts" ] && continue
    rm -f "$f"
done

# ── Step 2: Symlink bindings/bindings/*.ts → generated/*.ts ───────────────────

for src in "$BINDINGS_ROOT"/bindings/*.ts; do
    [ -f "$src" ] || continue
    link_file "$src" "$GENERATED_DIR/$(basename "$src")"
done

# ── Step 3: Symlink bindings/serde_json/*.ts → generated/serde_json/*.ts ──────

if [ -d "$BINDINGS_ROOT/serde_json" ]; then
    mkdir -p "$GENERATED_DIR/serde_json"
    # Clean stale
    for f in "$GENERATED_DIR"/serde_json/*.ts; do
        [ -e "$f" ] || [ -L "$f" ] || continue
        rm -f "$f"
    done
    for src in "$BINDINGS_ROOT"/serde_json/*.ts; do
        [ -f "$src" ] || continue
        link_file "$src" "$GENERATED_DIR/serde_json/$(basename "$src")"
    done
fi

# ── Step 4: Auto-generate index.ts ────────────────────────────────────────────

INDEX="$GENERATED_DIR/index.ts"
{
    echo "// Auto-generated barrel file — do not edit manually."
    echo "// Run scripts/sync-bindings.sh to regenerate."
    echo ""

    # Top-level types
    for f in "$GENERATED_DIR"/*.ts; do
        name="$(basename "$f" .ts)"
        [ "$name" = "index" ] && continue
        [ -e "$f" ] || continue
        echo "export type { $name } from './$name';"
    done

    # Subdirectory types (e.g. serde_json/JsonValue)
    for f in "$GENERATED_DIR"/serde_json/*.ts; do
        [ -e "$f" ] || continue
        name="$(basename "$f" .ts)"
        echo "export type { $name } from './serde_json/$name';"
    done
} > "$INDEX"

count="$(grep -c '^export' "$INDEX")"
echo "Synced bindings: $count types exported in index.ts"
