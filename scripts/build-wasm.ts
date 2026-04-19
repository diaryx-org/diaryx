#!/usr/bin/env bun
import { $ } from "bun";
import { resolve, dirname, join } from "path";

// --- Paths ---
const SCRIPT_DIR = dirname(resolve(import.meta.path));
const WORKSPACE_ROOT = dirname(SCRIPT_DIR);
const WEB_DIR = join(WORKSPACE_ROOT, "apps/web");
const WASM_OUT_DIR = join(WEB_DIR, "src/lib/wasm");
const WASM_FILE = join(WASM_OUT_DIR, "diaryx_wasm_bg.wasm");
const BINDINGS_DIR = join(WORKSPACE_ROOT, "crates/diaryx_core/bindings");

console.log(`Building WASM from workspace: ${WORKSPACE_ROOT}`);

// --- Verify wasm-pack ---
const wasmPackPath = await $`which wasm-pack`.text().catch(() => "");
if (!wasmPackPath.trim()) {
  console.error("Error: wasm-pack not found. Install it with: cargo install wasm-pack");
  process.exit(1);
}
console.log(`Using wasm-pack at: ${wasmPackPath.trim()}`);
console.log(`Building in directory: ${WORKSPACE_ROOT}/crates/diaryx_wasm`);

// --- macOS: propagate Xcode SDK settings ---
if (process.platform === "darwin") {
  const xcodeSelect = await $`which xcode-select`.text().catch(() => "");
  const xcrun = await $`which xcrun`.text().catch(() => "");

  if (xcodeSelect.trim() && xcrun.trim()) {
    if (!process.env.DEVELOPER_DIR) {
      const devDir = await $`xcode-select -p`.text().catch(() => "");
      if (devDir.trim()) process.env.DEVELOPER_DIR = devDir.trim();
    }

    if (!process.env.SDKROOT) {
      let sdkPath = await $`xcrun --sdk macosx --show-sdk-path`.text().catch(() => "");
      if (!sdkPath.trim()) {
        sdkPath = await $`xcrun --show-sdk-path`.text().catch(() => "");
      }
      if (sdkPath.trim()) process.env.SDKROOT = sdkPath.trim();
    }

    process.env.CC = "/usr/bin/cc";
    process.env.CXX = "/usr/bin/c++";
    process.env.AR = "/usr/bin/ar";
    process.env.CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER = "/usr/bin/cc";
  }
}

// --- Build WASM ---
$.cwd(WORKSPACE_ROOT);
await $`wasm-pack build crates/diaryx_wasm --target web --out-dir ${WASM_OUT_DIR}`;

// --- wasm-opt ---
const wasmOptPath = await $`which wasm-opt`.text().catch(() => "");
if (wasmOptPath.trim()) {
  console.log(`Running wasm-opt -Oz on ${WASM_FILE}`);
  await $`wasm-opt -Oz -o ${WASM_FILE} ${WASM_FILE}`;
} else {
  console.log("wasm-opt not found, skipping additional size optimization");
}

// --- Clean trailing whitespace in ts-rs bindings ---
const bindingsDir = Bun.file(BINDINGS_DIR);
if (await bindingsDir.exists()) {
  const glob = new Bun.Glob("**/*.ts");
  for await (const file of glob.scan(BINDINGS_DIR)) {
    const fullPath = join(BINDINGS_DIR, file);
    const content = await Bun.file(fullPath).text();
    const cleaned = content
      .split("\n")
      .map((line) => line.trimEnd())
      .join("\n");
    await Bun.write(fullPath, cleaned);
  }
  console.log("Cleaned trailing whitespace in ts-rs bindings");
}
