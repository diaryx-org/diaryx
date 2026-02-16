import { defineConfig } from "vite";

export default defineConfig({
  base: "./", // Relative paths so file:// loading works in WKWebView
  build: {
    outDir: "dist",
    // Inline all assets so we get a single folder with index.html + JS/CSS
    assetsInlineLimit: 0,
    rollupOptions: {
      output: {
        // Predictable filenames for easy embedding
        entryFileNames: "editor.js",
        assetFileNames: "[name][extname]",
      },
    },
  },
});
