/// Workaround plugin: resolve export conditions for the Svelte ecosystem.
///
/// Bun's dev server bundler currently ignores export conditions set by plugins
/// (see FIXME in bun-plugin-svelte). This plugin intervenes when a package
/// has a "svelte", "browser", or nested "import" condition that resolves to a
/// DIFFERENT file than what Bun's "default" resolution would use.
/// For all other packages, Bun's native resolution is used.

import type { BunPlugin } from "bun";
import { join, dirname } from "node:path";

/**
 * Resolve nested condition values.
 * e.g. { import: { types: "...", default: "./dist/foo.mjs" } }
 */
function resolveConditionValue(value: any): string | null {
  if (typeof value === "string") return value;
  if (typeof value === "object" && value !== null) {
    // Prefer import (ESM) over default (might be UMD/CJS)
    if (typeof value.import === "string") return value.import;
    if (typeof value.default === "string") return value.default;
    // Nested: { import: { default: "./foo.mjs" } }
    if (value.import && typeof value.import === "object" && typeof value.import.default === "string") {
      return value.import.default;
    }
  }
  return null;
}

/**
 * Check if a package's exports need intervention for correct ESM resolution.
 *
 * Handles three cases:
 * 1. "svelte" condition → prefer over default (Svelte ecosystem packages)
 * 2. "browser" condition → prefer over default (e.g. svelte's own exports)
 * 3. Nested "import" condition → prefer over top-level "default" when they
 *    resolve to different files (e.g. @floating-ui/* where "default" is UMD
 *    but "import.default" is ESM .mjs)
 *
 * Returns the preferred path if intervention is needed, null otherwise.
 */
function getPreferredExport(exports: any, subpath: string): string | null {
  if (!exports || typeof exports !== "object") return null;

  const entry = exports[subpath];
  if (!entry || typeof entry !== "object") return null;

  // What Bun's native resolver picks: the top-level "default" condition
  const topDefault = typeof entry.default === "string" ? entry.default : null;

  // 1. Svelte or browser condition (highest priority)
  const svelteResolved = resolveConditionValue(entry.svelte);
  const browserResolved = resolveConditionValue(entry.browser);
  const svelteOrBrowser = svelteResolved ?? browserResolved;

  if (svelteOrBrowser) {
    // Intervene if it differs from default, or there is no default
    if (!topDefault || svelteOrBrowser !== topDefault) {
      return svelteOrBrowser;
    }
    return null; // Same file — no intervention needed
  }

  // 2. Nested "import" condition (e.g. @floating-ui packages)
  //    { import: { default: "./dist/foo.mjs" }, default: "./dist/foo.umd.js" }
  if (entry.import && typeof entry.import === "object") {
    const importResolved = typeof entry.import.default === "string"
      ? entry.import.default
      : (typeof entry.import === "string" ? entry.import : null);
    if (importResolved && topDefault && importResolved !== topDefault) {
      return importResolved;
    }
  }

  // 3. No default at all but has an import condition (svelte-only packages)
  if (!topDefault) {
    const importResolved = resolveConditionValue(entry.import);
    if (importResolved) return importResolved;
  }

  return null; // Let Bun handle it normally
}

/**
 * Find a package's package.json using Node.js-style resolution:
 * Walk up from the importer's directory, checking node_modules at each level.
 * This correctly finds nested dependencies (e.g. svelte-sonner/node_modules/runed).
 */
function findPackageJson(specifier: string, importer?: string): string | null {
  try {
    const candidates: string[] = [];
    const root = process.cwd();

    if (importer) {
      let dir = dirname(importer);
      // Walk up directory tree, checking node_modules at each level
      while (dir.length >= root.length) {
        candidates.push(join(dir, "node_modules", specifier, "package.json"));
        const parent = dirname(dir);
        if (parent === dir) break; // reached filesystem root
        dir = parent;
      }
    }

    // Always include the project root as final fallback
    candidates.push(join(root, "node_modules", specifier, "package.json"));

    for (const candidate of candidates) {
      try {
        if (Bun.file(candidate).size > 0) return candidate;
      } catch {
        continue;
      }
    }
    return null;
  } catch {
    return null;
  }
}

const ROOT = join(process.cwd());

/**
 * Workaround: Bun's bundler doesn't handle `export * as Name from "./file.js"`
 * (namespace re-exports) correctly when used inside barrel files.
 *
 * bits-ui uses this pattern: `export * as Dialog from "./exports.js"`
 * When vaul-svelte does `import { Dialog } from "bits-ui"`, Bun fails to
 * resolve the namespace. We rewrite these imports to point directly at the
 * namespace source module.
 */
const BITS_UI_NAMESPACE_REWRITES: Record<string, string> = {};

// Build the rewrite map by scanning bits-ui's barrel structure
try {
  const bitsDir = join(ROOT, "node_modules/bits-ui/dist/bits");
  const bitsIndex = Bun.file(join(bitsDir, "index.js"));
  const bitsContent = await bitsIndex.text();
  // Match: export { Name } from "./name/index.js" or export * as Name from ...
  // We need to find the ones that use `export * as Name`
  const re = /export\s*\{\s*(\w+)\s*\}\s*from\s*"\.\/([^"]+)"/g;
  let match;
  while ((match = re.exec(bitsContent)) !== null) {
    const name = match[1];
    const subpath = match[2];
    const subIndex = join(bitsDir, subpath);
    try {
      const subContent = await Bun.file(subIndex).text();
      // Check if it uses `export * as Name from "./exports.js"`
      const nsMatch = subContent.match(/export\s*\*\s*as\s*\w+\s*from\s*"([^"]+)"/);
      if (nsMatch) {
        const exportsFile = join(dirname(subIndex), nsMatch[1]);
        BITS_UI_NAMESPACE_REWRITES[name] = exportsFile;
      }
    } catch {}
  }
} catch {}

const plugin: BunPlugin = {
  name: "svelte-export-conditions",
  setup(builder) {
    // Handle Vite's ?url import suffix — return a module that exports the file URL
    builder.onResolve({ filter: /\?url$/ }, (args) => {
      const realPath = args.path.replace(/\?url$/, "");
      return {
        path: realPath,
        namespace: "url-import",
      };
    });
    builder.onLoad({ filter: /.*/, namespace: "url-import" }, async (args) => {
      const resolved = Bun.resolveSync(args.path, ROOT);
      const relativePath = resolved.replace(ROOT, "");
      return {
        contents: `export default ${JSON.stringify(relativePath)};`,
        loader: "js",
      };
    });

    // Rewrite `import { Dialog } from "bits-ui"` → `import * as Dialog from "<direct-path>"`
    // inside node_modules files, to work around Bun's broken `export * as` namespace re-exports
    if (Object.keys(BITS_UI_NAMESPACE_REWRITES).length > 0) {
      builder.onLoad({ filter: /node_modules\/.*\.(js|svelte)$/ }, async (args) => {
        const source = await Bun.file(args.path).text();
        if (!source.includes('from "bits-ui"') && !source.includes("from 'bits-ui'")) return;

        let modified = source;
        for (const [name, exportsPath] of Object.entries(BITS_UI_NAMESPACE_REWRITES)) {
          // Match: import { Dialog as Foo } from "bits-ui" or import { Dialog } from "bits-ui"
          const importRe = new RegExp(
            `import\\s*\\{\\s*${name}(?:\\s+as\\s+(\\w+))?\\s*(?:,\\s*[^}]*)?\\}\\s*from\\s*["']bits-ui["']`,
            "g"
          );
          modified = modified.replace(importRe, (match, alias) => {
            const localName = alias || name;
            return `import * as ${localName} from "${exportsPath}"`;
          });
        }

        if (modified === source) return; // no changes
        return {
          contents: modified,
          loader: args.path.endsWith(".svelte") ? "js" : "js",
        };
      });
    }

    // Only intercept bare specifiers for packages that need svelte/browser conditions
    builder.onResolve({ filter: /^[^./]/ }, async (args) => {
      const parts = args.path.match(/^(@[^/]+\/[^/]+|[^/]+)(\/.*)?$/);
      if (!parts) return;

      const pkgName = parts[1];
      const subpath = parts[2] ? `.${parts[2]}` : ".";

      const pkgJsonPath = findPackageJson(pkgName, args.importer);
      if (!pkgJsonPath) return;

      try {
        const pkgJson = await Bun.file(pkgJsonPath).json();
        const exports = pkgJson.exports;
        if (!exports) return;

        const preferred = getPreferredExport(exports, subpath);
        if (!preferred) return; // Let Bun handle it normally

        const pkgDir = dirname(pkgJsonPath);
        return { path: join(pkgDir, preferred) };
      } catch {
        return;
      }
    });
  },
};

export default plugin;
