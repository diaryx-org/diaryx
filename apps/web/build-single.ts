/// Build Diaryx into a single self-contained HTML file
/// Usage: bun build-single.ts
///
/// Bundles everything into one HTML file with inline <script> and <style>.
/// External dependencies: Google Fonts CDN, WASM binary (loaded at runtime).
///
/// Note: Bun's native --compile --target=browser can't be used here because
/// readable-stream (transitive dep via zip.js) uses require('stream').
/// Instead we build without splitting and inline the output post-build.

import { join } from "node:path";
import { existsSync, rmSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import sveltePlugin from "./bun-plugins/svelte.ts";
import conditionsPlugin from "./bun-plugins/svelte-conditions.ts";
import viteEnvPlugin from "./bun-plugins/vite-env.ts";
import tailwind from "bun-plugin-tailwind";

const ROOT = import.meta.dir;
const OUTDIR = join(ROOT, "dist-single");
const pkg = await Bun.file(join(ROOT, "package.json")).json();

if (existsSync(OUTDIR)) rmSync(OUTDIR, { recursive: true });

console.log(`Building single-file Diaryx v${pkg.version}...`);
const start = performance.now();

const result = await Bun.build({
  entrypoints: [join(ROOT, "index.bun.html")],
  outdir: OUTDIR,
  target: "browser",
  format: "esm",
  splitting: false,
  sourcemap: "none",
  conditions: ["svelte", "browser"],
  minify: true,
  drop: ["console", "debugger"],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    "import.meta.env.DEV": "false",
    "import.meta.env.PROD": "true",
    "import.meta.env.MODE": '"production"',
    "import.meta.env.SSR": "false",
    "import.meta.env.BASE_URL": '"/"',
  },
  env: "VITE_*",
  plugins: [sveltePlugin, tailwind, conditionsPlugin, viteEnvPlugin],
});

if (!result.success) {
  console.error("Build failed");
  for (const log of result.logs) console.error(log);
  process.exit(1);
}

// Inline all JS and CSS into the HTML
let html = readFileSync(join(OUTDIR, "index.bun.html"), "utf-8");

// Inline CSS: <link rel="stylesheet" ... href="./chunk-xxx.css">
html = html.replace(
  /<link\s[^>]*href="\.\/([^"]+\.css)"[^>]*>/g,
  (_match, file) => {
    const filePath = join(OUTDIR, file);
    if (!existsSync(filePath)) return _match;
    return `<style>${readFileSync(filePath, "utf-8")}</style>`;
  },
);

// Inline JS: <script ... src="./chunk-xxx.js"></script>
html = html.replace(
  /<script\s[^>]*src="\.\/([^"]+\.js)"[^>]*><\/script>/g,
  (_match, file) => {
    const filePath = join(OUTDIR, file);
    if (!existsSync(filePath)) return _match;
    return `<script type="module">${readFileSync(filePath, "utf-8")}</script>`;
  },
);

// Inline favicon/icons as data URIs
html = html.replace(
  /<link\s[^>]*href="\.\/([^"]+\.(ico|png|svg))"[^>]*>/g,
  (match, file, ext) => {
    const filePath = join(OUTDIR, file);
    if (!existsSync(filePath)) return match;
    const mime =
      ext === "ico" ? "image/x-icon" :
      ext === "png" ? "image/png" :
      ext === "svg" ? "image/svg+xml" : "application/octet-stream";
    const b64 = readFileSync(filePath).toString("base64");
    return match.replace(`"./${file}"`, `"data:${mime};base64,${b64}"`);
  },
);

// Write the single file
const outPath = join(OUTDIR, "diaryx.html");
await Bun.write(outPath, html);

const elapsed = ((performance.now() - start) / 1000).toFixed(2);
const buf = readFileSync(outPath);
const gzSize = gzipSync(buf).byteLength;

console.log(`\n✓ Built in ${elapsed}s`);
console.log(`  ${(buf.byteLength / 1024).toFixed(0)} KB → ${(gzSize / 1024).toFixed(0)} KB gzip`);
console.log(`  Output: ${outPath}`);
