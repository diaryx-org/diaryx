# Diaryx command runner
# Run `just --list` to see all available recipes

set dotenv-load := false

# ── Build ────────────────────────────────────────────────────────────────────

# Build the CLI binary
build-cli:
    cargo build -p diaryx

# Build the sync server via Nix (hermetic, release)
build-sync-server:
    nix build .#diaryx-sync-server

# Build WASM package via Nix (hermetic)
build-wasm:
    nix build .#wasm-package

# Generate TypeScript bindings via Nix
build-bindings:
    nix build .#ts-bindings

# Build the web frontend
build-web: build-wasm build-bindings
    cd apps/web && bun install && bunx vite build

# Build everything
build-all: build-cli build-sync-server build-wasm build-bindings build-web

# ── Dev ──────────────────────────────────────────────────────────────────────

# Start web dev server
dev:
    cd apps/web && bun install && bun run dev

# Build WASM in dev mode (uses wasm-pack via script, faster iteration)
dev-wasm:
    ./scripts/build-wasm.sh

# Sync TypeScript bindings into the web app
sync-bindings:
    ./scripts/sync-bindings.sh

# Sync version from README.md to all project files
sync-versions:
    ./scripts/sync-versions.sh

# ── Test ─────────────────────────────────────────────────────────────────────

# Run Rust tests for all crates
test-rust:
    cargo test

# Run Rust tests for a specific crate
test-crate crate:
    cargo test -p {{crate}}

# Run web app unit tests
test-web:
    cd apps/web && bun run test -- --run

# Run web app E2E tests
test-e2e:
    cd apps/web && bunx playwright test

# Run all tests
test-all: test-rust test-web

# ── Lint ─────────────────────────────────────────────────────────────────────

# Format all Rust code
fmt:
    cargo fmt --all

# Check Rust formatting (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run Clippy lints
clippy:
    cargo clippy -p diaryx_core -- -D warnings

# Type-check the web app
check-web:
    cd apps/web && bun run check

# ── Deploy ───────────────────────────────────────────────────────────────────

# Deploy sync server to VPS
deploy-sync-server:
    @echo "Use the GitHub Actions workflow (deploy-sync-server.yml) for production deploys."
    @echo "For manual testing: nix build .#diaryx-sync-server && ls -la result/bin/"

# Deploy web app to Cloudflare
deploy-web:
    @echo "Use the GitHub Actions workflow (ci.yml deploy-web job) for production deploys."

# ── Apple ────────────────────────────────────────────────────────────────────

# Publish macOS app
publish-macos:
    ./scripts/publish-macos.sh

# Publish iOS app
publish-ios:
    ./scripts/publish-ios.sh

# ── Docs ─────────────────────────────────────────────────────────────────────

# Update the AGENTS.md workspace index
update-agents-index:
    ./scripts/update-agents-index.sh
