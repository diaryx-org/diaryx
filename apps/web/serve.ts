/// Bun fullstack dev server for Diaryx
/// Usage: bun serve.ts

import { join, extname } from "node:path";
import { existsSync, statSync } from "node:fs";
import homepage from "./index.bun.html";

const ROOT = import.meta.dir;
const PUBLIC_DIR = join(ROOT, "public");
const MARKETPLACE_DIST_DIR = join(ROOT, "marketplace-dist");
const CDN_ORIGIN = process.env.CDN_ORIGIN || "https://app.diaryx.org";

/** MIME types for static files */
const MIME: Record<string, string> = {
  ".wasm": "application/wasm",
  ".js": "application/javascript",
  ".mjs": "application/javascript",
  ".json": "application/json",
  ".html": "text/html",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".ico": "image/x-icon",
  ".webp": "image/webp",
  ".txt": "text/plain",
  ".xml": "application/xml",
  ".md": "text/markdown",
  ".zip": "application/zip",
};

/** Cross-origin isolation headers required for SharedArrayBuffer (WASM) */
const coiHeaders = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

/** Try to serve a static file, return null if not found */
function serveStaticFile(filePath: string): Response | null {
  if (!existsSync(filePath) || !statSync(filePath).isFile()) return null;
  const ext = extname(filePath).toLowerCase();
  const contentType = MIME[ext] || "application/octet-stream";
  return new Response(Bun.file(filePath), {
    headers: { "Content-Type": contentType, ...coiHeaders },
  });
}

/** Replace absolute CDN URLs with same-origin /cdn/ paths (for COEP compat) */
function rewriteCdnUrls(content: string): string {
  return content.replace(/https:\/\/cdn\.diaryx\.org\//g, "/cdn/");
}

/**
 * Serve /cdn/* from local marketplace-dist, falling back to production CDN.
 * Mirrors the localCdnPlugin from vite.config.ts.
 */
async function handleCdnRequest(relPath: string): Promise<Response> {
  const filePath = join(MARKETPLACE_DIST_DIR, relPath);

  // Security: ensure resolved path stays within marketplace-dist
  if (!filePath.startsWith(MARKETPLACE_DIST_DIR)) {
    return new Response("Forbidden", { status: 403 });
  }

  // Serve from local marketplace-dist if available
  if (existsSync(filePath) && statSync(filePath).isFile()) {
    const ext = extname(filePath).toLowerCase();
    const contentType = MIME[ext] || "application/octet-stream";

    // Rewrite CDN URLs in text registry files
    if (ext === ".md" || ext === ".json") {
      const content = await Bun.file(filePath).text();
      return new Response(rewriteCdnUrls(content), {
        headers: { "Content-Type": contentType, ...coiHeaders },
      });
    }

    return new Response(Bun.file(filePath), {
      headers: { "Content-Type": contentType, ...coiHeaders },
    });
  }

  // Proxy from production CDN for files not available locally
  try {
    const upstream = `${CDN_ORIGIN}/cdn/${relPath}`;
    const resp = await fetch(upstream);
    if (!resp.ok) {
      return new Response(resp.statusText, {
        status: resp.status,
        headers: coiHeaders,
      });
    }
    const contentType =
      resp.headers.get("content-type") || "application/octet-stream";
    const body = await resp.arrayBuffer();

    // Rewrite CDN URLs in proxied text responses
    if (contentType.includes("json") || contentType.includes("markdown") || contentType.includes("text")) {
      const text = new TextDecoder().decode(body);
      return new Response(rewriteCdnUrls(text), {
        headers: { "Content-Type": contentType, ...coiHeaders },
      });
    }

    return new Response(body, {
      headers: { "Content-Type": contentType, ...coiHeaders },
    });
  } catch (e: any) {
    return new Response(`CDN proxy error: ${e.message}`, {
      status: 502,
      headers: coiHeaders,
    });
  }
}

const server = Bun.serve({
  port: 5174,

  routes: {
    // HTML import — Bun bundles <script> and <link> tags automatically
    "/": homepage,
  },

  development: {
    // HMR disabled: Bun's HMR runtime has multiple incompatibilities:
    // 1. Template tracking: HMR wrapper breaks Svelte 5.55+ node indexing
    // 2. Namespace re-exports: `export * as Name` resolves to undefined
    // 3. Barrel imports: `export *` chains produce null namespace objects
    // Re-enable when Bun's HMR system is fixed for Svelte ecosystem packages.
    hmr: true,
    console: true,
  },

  async fetch(req) {
    const url = new URL(req.url);
    const pathname = url.pathname;

    // Proxy /api to the sync server
    if (pathname.startsWith("/api")) {
      const target = new URL(req.url);
      target.hostname = "localhost";
      target.port = "3030";
      return fetch(new Request(target, req));
    }

    // CDN / marketplace assets
    if (pathname.startsWith("/cdn/")) {
      const relPath = pathname.slice("/cdn/".length);
      return handleCdnRequest(relPath);
    }

    // Serve files from public/ directory (favicon, manifest, icons, etc.)
    const publicFile = serveStaticFile(join(PUBLIC_DIR, pathname));
    if (publicFile) return publicFile;

    // Serve WASM and other assets from src/
    if (pathname.startsWith("/src/")) {
      const srcFile = serveStaticFile(join(ROOT, pathname));
      if (srcFile) return srcFile;
    }

    // SPA fallback — serve the bundled HTML for client-side routing
    return new Response(Bun.file(join(ROOT, "index.bun.html")), {
      headers: { "Content-Type": "text/html", ...coiHeaders },
    });
  },
});

console.log(`Diaryx dev server running on ${server.url}`);
