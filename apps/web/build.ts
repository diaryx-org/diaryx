/// Bun production build script for Diaryx
/// Usage: bun build.ts
///
/// Mirrors the Vite build configuration with Bun's bundler.
/// Uses the same plugins as serve.ts (Svelte, Tailwind, export conditions, env).

import { join, basename, dirname } from "node:path";
import { cpSync, existsSync, rmSync, readdirSync, readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { gzipSync } from "node:zlib";
import sveltePlugin from "./bun-plugins/svelte.ts";
import conditionsPlugin from "./bun-plugins/svelte-conditions.ts";
import viteEnvPlugin from "./bun-plugins/vite-env.ts";
import tailwind from "bun-plugin-tailwind";

const ROOT = import.meta.dir;
const OUTDIR = join(ROOT, "dist");
const pkg = await Bun.file(join(ROOT, "package.json")).json();

// Clean output directory
if (existsSync(OUTDIR)) {
  rmSync(OUTDIR, { recursive: true });
}

console.log(`Building Diaryx v${pkg.version}...`);
const start = performance.now();

const result = await Bun.build({
  entrypoints: [join(ROOT, "index.bun.html")],
  outdir: OUTDIR,
  target: "browser",
  format: "esm",
  splitting: true,
  sourcemap: "none",
  conditions: ["svelte", "browser"],

  // Aggressive minification
  minify: {
    whitespace: true,
    syntax: true,
    identifiers: true,
  },

  // Drop debug/dev code
  drop: ["console", "debugger"],

  // Compile-time replacements
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    "import.meta.env.DEV": "false",
    "import.meta.env.PROD": "true",
    "import.meta.env.MODE": '"production"',
    "import.meta.env.SSR": "false",
    "import.meta.env.BASE_URL": '"/"',
  },

  // Inline VITE_ env vars
  env: "VITE_*",

  plugins: [sveltePlugin, tailwind, conditionsPlugin, viteEnvPlugin],

  naming: {
    entry: "[dir]/[name].[ext]",
    chunk: "assets/[name]-[hash].[ext]",
    asset: "assets/[name]-[hash].[ext]",
  },
});

if (!result.success) {
  console.error("Build failed:");
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}

// Copy public assets to output directory
const publicDir = join(ROOT, "public");
if (existsSync(publicDir)) {
  cpSync(publicDir, OUTDIR, { recursive: true });
}

// --- Post-build: deduplicate identical CSS chunks ---
// Bun's bundler emits duplicate CSS files for code-split chunks that share
// the same Tailwind utilities. We hash each CSS file, keep one canonical copy,
// and rewrite JS import references to point to it.
{
  const assetsDir = join(OUTDIR, "assets");
  if (existsSync(assetsDir)) {
    const cssFiles = readdirSync(assetsDir).filter((f) => f.endsWith(".css"));

    // Group CSS files by content hash
    const hashToFiles = new Map<string, string[]>();
    for (const file of cssFiles) {
      const content = readFileSync(join(assetsDir, file));
      const hash = Bun.hash(content).toString(36);
      const group = hashToFiles.get(hash) ?? [];
      group.push(file);
      hashToFiles.set(hash, group);
    }

    let deduped = 0;
    for (const [, files] of hashToFiles) {
      if (files.length <= 1) continue;

      // Keep the first file as canonical
      const canonical = files[0];
      const duplicates = files.slice(1);

      // Rewrite JS files that reference duplicates → canonical
      const jsFiles = readdirSync(assetsDir).filter((f) => f.endsWith(".js"));
      for (const jsFile of jsFiles) {
        const jsPath = join(assetsDir, jsFile);
        let jsContent = readFileSync(jsPath, "utf-8");
        let changed = false;
        for (const dup of duplicates) {
          if (jsContent.includes(dup)) {
            jsContent = jsContent.replaceAll(dup, canonical);
            changed = true;
          }
        }
        if (changed) writeFileSync(jsPath, jsContent);
      }

      // Also rewrite the HTML entry
      const htmlPath = join(OUTDIR, "index.bun.html");
      if (existsSync(htmlPath)) {
        let html = readFileSync(htmlPath, "utf-8");
        let changed = false;
        for (const dup of duplicates) {
          if (html.includes(dup)) {
            html = html.replaceAll(dup, canonical);
            changed = true;
          }
        }
        if (changed) writeFileSync(htmlPath, html);
      }

      // Delete duplicates
      for (const dup of duplicates) {
        unlinkSync(join(assetsDir, dup));
        deduped++;
      }
    }

    if (deduped > 0) {
      console.log(`  Deduped ${deduped} identical CSS chunks`);
    }
  }
}

// --- Build report (reads from disk after dedup) ---

const elapsed = ((performance.now() - start) / 1000).toFixed(2);
const assetsPath = join(OUTDIR, "assets");
const allAssets = existsSync(assetsPath) ? readdirSync(assetsPath) : [];
const jsFiles = allAssets.filter((f) => f.endsWith(".js"));
const cssFiles = allAssets.filter((f) => f.endsWith(".css"));

const fmt = (b: number) =>
  b >= 1024 * 1024
    ? `${(b / 1024 / 1024).toFixed(2)} MB`
    : `${(b / 1024).toFixed(0)} KB`;

// Compute sizes from disk
let jsRaw = 0, jsGzip = 0, cssRaw = 0, cssGzip = 0;

interface FileInfo { name: string; raw: number; gzip: number }
const jsInfo: FileInfo[] = [];

for (const f of jsFiles) {
  const buf = readFileSync(join(assetsPath, f));
  const gz = gzipSync(buf).byteLength;
  jsRaw += buf.byteLength;
  jsGzip += gz;
  jsInfo.push({ name: `assets/${f}`, raw: buf.byteLength, gzip: gz });
}
for (const f of cssFiles) {
  const buf = readFileSync(join(assetsPath, f));
  cssRaw += buf.byteLength;
  cssGzip += gzipSync(buf).byteLength;
}

console.log(`\n✓ Built in ${elapsed}s`);
console.log(`  JS:  ${fmt(jsRaw)} → ${fmt(jsGzip)} gzip (${jsFiles.length} files)`);
console.log(`  CSS: ${fmt(cssRaw)} → ${fmt(cssGzip)} gzip (${cssFiles.length} files)`);
console.log(`  Output: ${OUTDIR}`);

// Show largest chunks
jsInfo.sort((a, b) => b.raw - a.raw);
console.log("\n  Largest JS chunks:");
for (const o of jsInfo.slice(0, 8)) {
  console.log(`    ${fmt(o.raw).padStart(10)} → ${fmt(o.gzip).padStart(7)} gzip  ${o.name}`);
}
