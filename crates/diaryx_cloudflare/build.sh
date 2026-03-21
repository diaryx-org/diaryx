#!/usr/bin/env bash
set -euo pipefail

cargo install -q worker-build

# worker-build's internal esbuild doesn't mark Workers runtime imports
# as external, causing bundling to fail. Run worker-build and, if its
# esbuild step fails, re-bundle ourselves with the correct externals.
if worker-build --release 2>&1; then
  exit 0
fi

# The wasm compilation succeeded — only the esbuild step failed.
# The intermediate JS glue is in build/.tmp/. Re-bundle with externals.

# wasm-bindgen 0.2.114+ emits `import source ... from "*.wasm"` (source
# phase imports), which esbuild doesn't support. Rewrite to a regular
# import so esbuild can process the file.
sed 's/import source wasmModule/import wasmModule/' build/.tmp/index.js > build/.tmp/index.js.tmp
mv build/.tmp/index.js.tmp build/.tmp/index.js

mkdir -p build/worker
npx --yes esbuild@0.27 build/.tmp/shim.js \
  --bundle \
  --format=esm \
  --external:env \
  --external:cloudflare:workers \
  --external:./index_bg.wasm \
  --outfile=build/worker/shim.mjs \
  --minify

cp build/.tmp/index_bg.wasm build/worker/index_bg.wasm
