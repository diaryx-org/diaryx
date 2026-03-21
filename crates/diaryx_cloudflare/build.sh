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

# getrandom 0.3 with the `custom` feature (enabled transitively) generates
# a wasm import `env.__getrandom_v03_custom`. Provide a JS shim that
# implements it using Web Crypto (available in Workers).
cat > build/.tmp/env.js << 'ENVJS'
// Lazily resolved wasm memory — not available until after instantiation,
// but __getrandom_v03_custom is only called at runtime, not during init.
let memory;
export function __getrandom_v03_custom(ptr, len) {
  if (!memory) {
    // The wasm-bindgen glue exports `wasm` which has `.memory`.
    // At call time the instance is fully initialised.
    throw new Error("getrandom called before wasm memory is available");
  }
  const buf = new Uint8Array(memory.buffer, ptr, len);
  crypto.getRandomValues(buf);
  return 0; // success
}
// Called by the patched glue to hand over the memory reference.
export function __set_memory(mem) { memory = mem; }
ENVJS

# Patch index.js to hand the wasm memory to our env shim after instantiation.
# Insert `import1.__set_memory(wasm.memory);` right after `wasm = wasmInstance.exports;`
sed 's/wasm = wasmInstance\.exports;/wasm = wasmInstance.exports; if (typeof import1.__set_memory === "function") import1.__set_memory(wasm.memory);/' \
  build/.tmp/index.js > build/.tmp/index.js.tmp
mv build/.tmp/index.js.tmp build/.tmp/index.js

mkdir -p build/worker
npx --yes esbuild@0.27 build/.tmp/shim.js \
  --bundle \
  --format=esm \
  --alias:env=./build/.tmp/env.js \
  --external:cloudflare:workers \
  --external:./index_bg.wasm \
  --outfile=build/worker/shim.mjs \
  --minify

cp build/.tmp/index_bg.wasm build/worker/index_bg.wasm
