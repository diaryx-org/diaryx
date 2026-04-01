/// Custom bun-plugin-svelte wrapper.
///
/// Wraps the official plugin to:
/// 1. Force client-side generation (forceSide: "client")
/// 2. Replace import.meta.env.* references in compiled Svelte output
///
/// HMR is disabled at the Bun.serve() level (hmr: false in serve.ts)
/// because the HMR wrapper is incompatible with Svelte 5.55+.

import type { BunPlugin, OnLoadResult } from "bun";
import { basename, join } from "node:path";
import { compile, compileModule } from "svelte/compiler";

// Inline the hash helper from bun-plugin-svelte
const hash = (content: string): string => Bun.hash(content, 5381).toString(36);

// Read app version from package.json (mirrors Vite's define: { __APP_VERSION__ })
const pkg = await Bun.file(join(import.meta.dir, "../package.json")).json();

// Compile-time replacements (import.meta.env + Vite defines)
const ENV_REPLACEMENTS: [RegExp, string][] = [
  [/\b__APP_VERSION__\b/g, JSON.stringify(pkg.version)],
  [/\bimport\.meta\.env\.DEV\b/g, "true"],
  [/\bimport\.meta\.env\.PROD\b/g, "false"],
  [/\bimport\.meta\.env\.MODE\b/g, '"development"'],
  [/\bimport\.meta\.env\.SSR\b/g, "false"],
  [/\bimport\.meta\.env\.BASE_URL\b/g, '"/"'],
];

for (const [key, value] of Object.entries(process.env)) {
  if (key.startsWith("VITE_") && value !== undefined) {
    ENV_REPLACEMENTS.push([
      new RegExp(`\\bimport\\.meta\\.env\\.${key}\\b`, "g"),
      JSON.stringify(value),
    ]);
  }
}

function replaceEnv(code: string): string {
  for (const [pattern, replacement] of ENV_REPLACEMENTS) {
    code = code.replace(pattern, replacement);
  }
  return code;
}

const virtualNamespace = "bun-svelte";

const plugin: BunPlugin = {
  name: "bun-plugin-svelte-custom",
  setup(builder) {
    // Push svelte export condition (same as official plugin)
    if (builder?.config) {
      let conditions = builder.config.conditions ?? [];
      if (typeof conditions === "string") conditions = [conditions];
      conditions.push("svelte");
      builder.config.conditions = conditions;
    }

    const virtualCssModules = new Map<string, { sourcePath: string; source: string }>();

    builder
      .onLoad({ filter: /\.svelte$/ }, async (args) => {
        const sourceText = await Bun.file(args.path).text();

        const result = compile(sourceText, {
          css: "external",
          generate: "client",
          filename: args.path,
          hmr: false,
          dev: false,
          cssHash({ css }: { css: string }) {
            return `svelte-${hash(css)}`;
          },
        });

        let jsCode = result.js.code;

        // Extract CSS into virtual module
        if (result.css?.code) {
          const uid = `${basename(args.path)}-${hash(args.path)}-style`.replaceAll(`"`, `'`);
          const virtualName = virtualNamespace + ":" + uid + ".css";
          virtualCssModules.set(virtualName, { sourcePath: args.path, source: result.css.code });
          jsCode += `\nimport "${virtualName}";`;
        }

        // Replace import.meta.env references in compiled output
        jsCode = replaceEnv(jsCode);

        return {
          contents: jsCode,
          loader: "js",
        } satisfies OnLoadResult;
      })
      .onLoad({ filter: /\.svelte\.[tj]s$/ }, async (args) => {
        let sourceText = await Bun.file(args.path).text();
        if (args.path.endsWith(".ts")) {
          const ts = new Bun.Transpiler({ loader: "ts" });
          sourceText = await ts.transform(sourceText);
        }
        const result = compileModule(sourceText, {
          dev: false,
          generate: "client",
          filename: args.path,
        });
        return {
          contents: replaceEnv(result.js.code),
          loader: "js",
        };
      })
      .onResolve({ filter: /^bun-svelte:/ }, (args) => ({
        path: args.path,
        namespace: "bun-svelte",
      }))
      .onLoad({ filter: /\.css$/, namespace: virtualNamespace }, (args) => {
        const mod = virtualCssModules.get(args.path);
        if (!mod) throw new Error("Virtual CSS module not found: " + args.path);
        virtualCssModules.delete(args.path);
        return {
          contents: mod.source,
          loader: "css",
          watchFiles: [mod.sourcePath],
        };
      });
  },
};

export default plugin;
