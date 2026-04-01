/// Plugin to replace Vite-specific import.meta.env.* references and
/// compile-time defines at bundle time.
///
/// Bun's dev server doesn't support builder.config.define, so we do a simple
/// string replacement in onLoad for .ts/.js files in the src/ directory.
/// (.svelte files are handled by our custom svelte plugin wrapper.)

import type { BunPlugin } from "bun";
import { join } from "node:path";

// Read app version from package.json (mirrors Vite's define: { __APP_VERSION__ })
const pkg = await Bun.file(join(import.meta.dir, "../package.json")).json();

const replacements: [RegExp, string][] = [
  // Vite defines
  [/\b__APP_VERSION__\b/g, JSON.stringify(pkg.version)],
  // import.meta.env
  [/\bimport\.meta\.env\.DEV\b/g, "true"],
  [/\bimport\.meta\.env\.PROD\b/g, "false"],
  [/\bimport\.meta\.env\.MODE\b/g, '"development"'],
  [/\bimport\.meta\.env\.SSR\b/g, "false"],
  [/\bimport\.meta\.env\.BASE_URL\b/g, '"/"'],
];

// Add VITE_* env vars
for (const [key, value] of Object.entries(process.env)) {
  if (key.startsWith("VITE_") && value !== undefined) {
    replacements.push([
      new RegExp(`\\bimport\\.meta\\.env\\.${key}\\b`, "g"),
      JSON.stringify(value),
    ]);
  }
}

function applyReplacements(source: string): string {
  let result = source;
  for (const [pattern, replacement] of replacements) {
    result = result.replace(pattern, replacement);
  }
  return result;
}

const plugin: BunPlugin = {
  name: "vite-env-shim",
  setup(builder) {
    // Transform .ts and .js files in src/ that reference import.meta.env
    builder.onLoad({ filter: /\/src\/.*\.[tj]s$/ }, async (args) => {
      const source = await Bun.file(args.path).text();
      if (!source.includes("import.meta.env") && !source.includes("__APP_VERSION__")) return;

      return {
        contents: applyReplacements(source),
        loader: args.path.endsWith(".ts") ? "ts" : "js",
      };
    });
  },
};

export default plugin;
