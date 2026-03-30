import { isTauri } from "$lib/backend/interface";

const DEFAULT_REMOTE_CDN_BASE_URL = "https://app.diaryx.org/cdn";

/**
 * CDN base URL for marketplace assets (themes, plugins, bundles, etc.).
 *
 * Web builds default to same-origin `/cdn` so the worker/dev server can serve
 * curated assets locally. Tauri builds need an absolute URL for any native
 * proxy fetches because reqwest cannot resolve relative paths like `/cdn/...`.
 *
 * Set `VITE_CDN_BASE_URL` to override (for example `https://app.diaryx.org/cdn`).
 */
export function resolveCdnBaseUrl(): string {
  const envUrl = (import.meta as any).env?.VITE_CDN_BASE_URL;
  if (typeof envUrl === "string" && envUrl.length > 0) {
    return envUrl.replace(/\/$/, "");
  }

  if (typeof window === "undefined" || !isTauri()) {
    return "/cdn";
  }

  if (window.location.protocol === "http:" || window.location.protocol === "https:") {
    return new URL("/cdn", window.location.origin).toString().replace(/\/$/, "");
  }

  return DEFAULT_REMOTE_CDN_BASE_URL;
}

export const CDN_BASE_URL: string = resolveCdnBaseUrl();
