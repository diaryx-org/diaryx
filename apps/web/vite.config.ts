import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import basicSsl from "@vitejs/plugin-basic-ssl";
import path from "path";
import { readFileSync } from "fs";

const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf-8"),
);
const isTauri = !!process.env.TAURI_ENV_PLATFORM;
const useWasmCdn = !!process.env.VITE_WASM_CDN_URL;
const tauriDevHost = process.env.TAURI_DEV_HOST;
const useHttps = !!process.env.VITE_HTTPS;
const canonicalDevHost = "localhost";
const enableCrossOriginIsolation = process.env.VITE_DISABLE_COI !== "1";

function readCliFlag(flagName: string): string | undefined {
  const exactIndex = process.argv.findIndex((arg) => arg === `--${flagName}`);
  if (exactIndex >= 0) {
    return process.argv[exactIndex + 1];
  }

  const prefixed = process.argv.find((arg) => arg.startsWith(`--${flagName}=`));
  return prefixed?.slice(flagName.length + 3);
}

const requestedDevPort = process.env.PW_WEB_PORT ?? readCliFlag("port");
const requestedDevHost = readCliFlag("host");
const explicitDevOrigin = process.env.PW_BASE_URL;
const devPort = Number(requestedDevPort ?? 5174);
const devOrigin = explicitDevOrigin
  ?? `${useHttps ? "https" : "http"}://${
    requestedDevHost && requestedDevHost !== "127.0.0.1"
      ? requestedDevHost
      : canonicalDevHost
  }:${devPort}`;

// https://vitejs.dev/config/
export default defineConfig({
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  plugins: [tailwindcss(), svelte() as any, ...(useHttps ? [basicSsl()] : [])],
  // Base path for GitHub Pages deployment
  // Set VITE_BASE_PATH env var to deploy to a subdirectory (e.g., "/repo-name/")
  base: process.env.VITE_BASE_PATH || "/",
  // Prevent vite from obscuring rust errors
  clearScreen: false,
  server: {
    port: devPort,
    strictPort: isTauri, // Tauri expects a fixed port
    host: isTauri ? tauriDevHost || false : requestedDevHost || canonicalDevHost,
    origin: isTauri
      ? undefined
      : devOrigin,
    hmr: tauriDevHost
      ? {
          protocol: "ws",
          host: tauriDevHost,
          port: 1421,
        }
      : undefined,
    headers: enableCrossOriginIsolation
      ? {
          "Cross-Origin-Opener-Policy": "same-origin",
          "Cross-Origin-Embedder-Policy": "require-corp",
        }
      : undefined,
    watch: {
      // Tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS and Linux
    target: isTauri
      ? process.env.TAURI_ENV_PLATFORM === "windows"
        ? "chrome105"
        : "safari13"
      : "es2020",
    // Don't minify for debug builds
    minify: isTauri && process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    // Produce sourcemaps for debug builds
    sourcemap: isTauri ? !!process.env.TAURI_ENV_DEBUG : true,
    rollupOptions: {
      onwarn(warning, defaultHandler) {
        // Suppress Svelte 5 compiler @__PURE__ annotation warnings — these
        // are compiler artifacts and not actionable in user code.
        if (
          warning.code === "INVALID_ANNOTATION" &&
          warning.message.includes("@__PURE__")
        ) {
          return;
        }
        defaultHandler(warning);
      },
      output: {
        manualChunks(id) {
          if (id.includes("node_modules")) {
            if (id.includes("@tiptap/") || id.includes("prosemirror-")) {
              return "vendor-tiptap";
            }
            if (id.includes("@extism/") || id.includes("@bjorn3/")) {
              return "vendor-extism";
            }
            if (id.includes("/svelte/")) {
              return "vendor-svelte";
            }
            if (id.includes("bits-ui")) {
              return "vendor-ui";
            }
          }
        },
      },
    },
  },
  resolve: {
    alias: {
      // Stub out Tauri API for web builds - will be tree-shaken when not used
      "@tauri-apps/api/core": "@tauri-apps/api/core",
      // In Tauri builds, stub out WASM (Tauri uses native Rust backend, not WASM).
      // When using CDN, also stub it out — the worker loads from CDN via dynamic import.
      "@diaryx/wasm": path.resolve(
        isTauri || useWasmCdn
          ? "./src/lib/wasm-stub.js"
          : "./src/lib/wasm/diaryx_wasm.js",
      ),
      $lib: path.resolve("./src/lib"),
      "@": path.resolve(__dirname, "./src"),
    },
  },
  optimizeDeps: {
    // Exclude Tauri API from optimization since it's optional
    exclude: ["@tauri-apps/api", "@diaryx/wasm"],
    include: ["@bjorn3/browser_wasi_shim"],
  },
  // Env variables starting with the item of `envPrefix` will be exposed in tauri's source code through `import.meta.env`.
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  worker: {
    format: "es",
  },
});
