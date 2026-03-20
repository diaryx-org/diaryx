/**
 * CDN base URL for marketplace assets (themes, plugins, bundles, etc.).
 *
 * Defaults to same-origin `/cdn` so the web worker can serve from R2 directly.
 * Set VITE_CDN_BASE_URL to override (e.g. "https://cdn.diaryx.org").
 */
export const CDN_BASE_URL: string =
  (import.meta as any).env?.VITE_CDN_BASE_URL || "/cdn";
