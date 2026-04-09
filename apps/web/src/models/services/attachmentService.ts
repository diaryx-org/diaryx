/**
 * Attachment Service
 *
 * Manages blob URLs for displaying attachments in the editor.
 * Handles transforming file paths to blob URLs and back.
 */

import type { Api } from '$lib/backend/api';
import { isTauri } from '$lib/backend/interface';
import { parseLinkDisplay } from '$lib/utils/linkParser';
import { getServerAttachmentUrl, getAttachmentMetadata, sha256Hex } from '$lib/sync/attachmentSyncService';

// ============================================================================
// State
// ============================================================================

// Blob URL tracking for attachments (originalPath -> blobUrl)
const blobUrlMap = new Map<string, string>();

// Thumbnail URL cache keyed by attachment hash or source path.
// This keeps picker thumbnails reusable across surfaces without holding
// on to unbounded object URLs.
const thumbnailUrlMap = new Map<string, string>();
const thumbnailPromiseMap = new Map<string, Promise<string | undefined>>();
const THUMBNAIL_CACHE_LIMIT = 96;
const THUMBNAIL_MAX_DIMENSION_PX = 240;

// Positive verification cache for synced local attachments keyed by
// storage path + expected hash. This lets us skip repeat hashing after a
// verified local read while still invalidating naturally when metadata changes.
const verifiedAttachmentMap = new Map<string, true>();
const VERIFIED_ATTACHMENT_CACHE_LIMIT = 256;

// Normalization cache to avoid repeated WASM round-trips ("entryPath::rawPath" -> result)
const normalizationCache = new Map<string, { canonical: string; sourceRelative: string }>();

// Abort controller for in-flight resolveAttachment calls.
// Aborted on entry switch so stale resolutions don't saturate the WASM worker queue.
let resolveAbort = new AbortController();

// Concurrency-limited resolution queue.
// Keep this intentionally low: attachment reads share a backend command lane with
// entry navigation, so high concurrency can make file switches feel blocked.
// On entry switch, the JS queue is flushed instantly — at most RESOLVE_CONCURRENCY
// in-flight calls need to finish (they bail fast via abort checks).
const RESOLVE_CONCURRENCY = 1;
let resolveInFlight = 0;
interface PendingResolve { start: () => void; cancel: () => void }
const pendingResolves: PendingResolve[] = [];

function drainPendingResolves(): void {
  while (resolveInFlight < RESOLVE_CONCURRENCY && pendingResolves.length > 0) {
    resolveInFlight++;
    pendingResolves.shift()!.start();
  }
}

function getThumbnailCacheKey(sourceEntryPath: string, attachmentPath: string): string {
  const metadata = getAttachmentMetadata(sourceEntryPath, attachmentPath);
  if (metadata?.hash) return `hash:${metadata.hash}`;
  return `path:${sourceEntryPath}::${attachmentPath}`;
}

function getResolvedAttachmentMetadata(
  entryPath: string,
  canonicalPath: string,
  sourceRelativePath: string,
) {
  return (
    getAttachmentMetadata(entryPath, sourceRelativePath) ||
    (canonicalPath !== sourceRelativePath
      ? getAttachmentMetadata(entryPath, canonicalPath)
      : null)
  );
}

function touchThumbnailCacheEntry(key: string): string | undefined {
  const existing = thumbnailUrlMap.get(key);
  if (!existing) return undefined;
  thumbnailUrlMap.delete(key);
  thumbnailUrlMap.set(key, existing);
  return existing;
}

function evictThumbnailCacheIfNeeded(): void {
  while (thumbnailUrlMap.size > THUMBNAIL_CACHE_LIMIT) {
    const oldestKey = thumbnailUrlMap.keys().next().value;
    if (!oldestKey) break;
    const oldestUrl = thumbnailUrlMap.get(oldestKey);
    if (oldestUrl) {
      URL.revokeObjectURL(oldestUrl);
    }
    thumbnailUrlMap.delete(oldestKey);
  }
}

function getVerifiedAttachmentCacheKey(storagePath: string, expectedHash: string): string {
  return `${storagePath}::${expectedHash}`;
}

function touchVerifiedAttachmentCacheEntry(key: string): boolean {
  if (!verifiedAttachmentMap.has(key)) return false;
  verifiedAttachmentMap.delete(key);
  verifiedAttachmentMap.set(key, true);
  return true;
}

function rememberVerifiedAttachment(key: string): void {
  verifiedAttachmentMap.delete(key);
  verifiedAttachmentMap.set(key, true);

  while (verifiedAttachmentMap.size > VERIFIED_ATTACHMENT_CACHE_LIMIT) {
    const oldestKey = verifiedAttachmentMap.keys().next().value;
    if (!oldestKey) break;
    verifiedAttachmentMap.delete(oldestKey);
  }
}

async function verifyLocalAttachmentAgainstMetadata(
  api: Api,
  entryPath: string,
  canonicalPath: string,
  sourceRelativePath: string,
  storagePath: string,
  bytes?: Uint8Array,
  signal?: AbortSignal,
): Promise<'ok' | 'stale' | 'aborted'> {
  const meta = getResolvedAttachmentMetadata(entryPath, canonicalPath, sourceRelativePath);
  if (!meta?.hash) return 'ok';

  const verificationKey = getVerifiedAttachmentCacheKey(storagePath, meta.hash);
  if (touchVerifiedAttachmentCacheEntry(verificationKey)) {
    return 'ok';
  }

  const localBytes = bytes ?? await api.readBinary(storagePath);
  if (signal?.aborted) return 'aborted';

  const localHash = await sha256Hex(localBytes);
  if (signal?.aborted) return 'aborted';
  if (localHash !== meta.hash) return 'stale';

  rememberVerifiedAttachment(verificationKey);
  return 'ok';
}

async function resolveTauriNativePreviewUrlFromPaths(
  api: Api,
  entryPath: string,
  canonicalPath: string,
  sourceRelativePath: string,
): Promise<string | null> {
  if (!isTauri() || isHeicFile(canonicalPath)) return null;

  try {
    const storagePath = await api.resolveAttachmentStoragePath(entryPath, sourceRelativePath);
    if (!(await api.fileExists(storagePath))) return null;

    const verification = await verifyLocalAttachmentAgainstMetadata(
      api,
      entryPath,
      canonicalPath,
      sourceRelativePath,
      storagePath,
    );
    if (verification !== 'ok') return null;

    const { convertFileSrc } = await import('@tauri-apps/api/core');
    return convertFileSrc(storagePath);
  } catch {
    return null;
  }
}

// ============================================================================
// MIME Type Mapping
// ============================================================================

const mimeTypes: Record<string, string> = {
  // Images
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  heic: 'image/heic',
  heif: 'image/heif',
  // Videos
  mp4: 'video/mp4',
  webm: 'video/webm',
  ogg: 'video/ogg',
  mov: 'video/quicktime',
  avi: 'video/x-msvideo',
  mkv: 'video/x-matroska',
  m4v: 'video/x-m4v',
  // Audio
  mp3: 'audio/mpeg',
  wav: 'audio/wav',
  flac: 'audio/flac',
  aac: 'audio/aac',
  m4a: 'audio/mp4',
  wma: 'audio/x-ms-wma',
  // Documents
  pdf: 'application/pdf',
  doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xls: 'application/vnd.ms-excel',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  ppt: 'application/vnd.ms-powerpoint',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  // Text
  txt: 'text/plain',
  html: 'text/html',
  htm: 'text/html',
  md: 'text/markdown',
  csv: 'text/csv',
  json: 'application/json',
  xml: 'application/xml',
  // Archives
  zip: 'application/zip',
  tar: 'application/x-tar',
  gz: 'application/gzip',
  '7z': 'application/x-7z-compressed',
  rar: 'application/vnd.rar',
};

export type AttachmentMediaKind = 'image' | 'video' | 'audio' | 'file';

/**
 * Get the MIME type for a file based on its extension.
 */
export function getMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return mimeTypes[ext] || 'application/octet-stream';
}

/**
 * Check if a file is a HEIC/HEIF image (Apple's format).
 */
export function isHeicFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ext === 'heic' || ext === 'heif';
}

/**
 * Check if a file is an image based on its extension.
 */
export function isImageFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'heic', 'heif'].includes(ext);
}

/**
 * Check if a file is a video based on its extension.
 */
export function isVideoFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ['mp4', 'webm', 'ogg', 'mov', 'avi', 'mkv', 'm4v'].includes(ext);
}

/**
 * Check if a file is an audio file based on its extension.
 */
export function isAudioFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg', 'wma'].includes(ext);
}

/**
 * Check if a file is an HTML file based on its extension.
 */
export function isHtmlFile(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return ['html', 'htm'].includes(ext);
}

/**
 * Classify an attachment by preview behavior.
 *
 * MIME type, when available, wins over extension-based inference so uploads
 * like `.ogg` can be disambiguated correctly.
 */
export function getAttachmentMediaKind(
  path: string,
  mimeType?: string | null,
): AttachmentMediaKind {
  const normalizedMimeType = mimeType?.trim().toLowerCase();
  if (normalizedMimeType?.startsWith('image/')) return 'image';
  if (normalizedMimeType?.startsWith('video/')) return 'video';
  if (normalizedMimeType?.startsWith('audio/')) return 'audio';

  if (isImageFile(path)) return 'image';
  if (isVideoFile(path)) return 'video';
  if (isAudioFile(path)) return 'audio';
  return 'file';
}

/**
 * Check whether an attachment kind can be previewed inline or in the preview dialog.
 */
export function isPreviewableAttachmentKind(kind: AttachmentMediaKind): boolean {
  return kind !== 'file';
}

/**
 * Check if a file is a displayable media file (image, video, or audio).
 */
export function isMediaFile(path: string): boolean {
  return isPreviewableAttachmentKind(getAttachmentMediaKind(path));
}

/**
 * Extract the filename from a path.
 */
export function getFilename(path: string): string {
  return path.split('/').pop() || path;
}

/**
 * Format a markdown destination so paths with whitespace remain valid CommonMark.
 */
export function formatMarkdownDestination(path: string): string {
  const trimmed = path.trim();
  if (trimmed.startsWith('<') && trimmed.endsWith('>')) {
    return path;
  }
  return /\s/u.test(path) ? `<${path}>` : path;
}

function normalizeSlashes(path: string): string {
  return path.replace(/\\/g, '/');
}

export function stripWorkspacePrefixFromAttachmentPath(
  rawPath: string,
  workspacePath?: string | null,
): string {
  const normalizedPath = unwrapAngleBracketPath(normalizeSlashes(rawPath).trim());
  if (!workspacePath) return normalizedPath;

  const normalizedWorkspacePath = normalizeSlashes(workspacePath).replace(/\/+$/, '');
  if (!normalizedWorkspacePath) return normalizedPath;

  const workspaceRoots = [normalizedWorkspacePath];
  if (/\/[^/]+\.md$/i.test(normalizedWorkspacePath)) {
    const lastSlash = normalizedWorkspacePath.lastIndexOf('/');
    if (lastSlash > 0) {
      workspaceRoots.unshift(normalizedWorkspacePath.slice(0, lastSlash));
    }
  }

  const normalizedCandidate = normalizedPath.replace(/^\/+/, '');
  for (const root of workspaceRoots) {
    const normalizedRoot = root.replace(/^\/+/, '').replace(/\/+$/, '');
    if (!normalizedRoot) continue;
    if (normalizedCandidate === normalizedRoot) return normalizedPath;
    if (normalizedCandidate.startsWith(`${normalizedRoot}/`)) {
      return normalizedCandidate.slice(normalizedRoot.length + 1);
    }
  }

  return normalizedPath;
}

export async function formatDroppedAttachmentPathForEntry(
  api: Api | null,
  targetEntryPath: string,
  attachmentRaw: string,
  options: {
    sourceEntryPath?: string;
    workspacePath?: string | null;
  } = {},
): Promise<{ path: string; label: string }> {
  const parsed = parseLinkDisplay(attachmentRaw);
  const rawPath = parsed?.path ?? attachmentRaw;
  const strippedPath = stripWorkspacePrefixFromAttachmentPath(rawPath, options.workspacePath);
  const fallbackLabel = parsed?.title?.trim() || getFilename(strippedPath) || 'attachment';

  if (!api || !targetEntryPath) {
    return { path: strippedPath, label: fallbackLabel };
  }

  try {
    const canonical = await api.canonicalizeLink(
      strippedPath,
      options.sourceEntryPath || targetEntryPath,
    );
    const formatted = await api.formatLink(
      canonical,
      getFilename(canonical) || fallbackLabel,
      'plain_relative',
      targetEntryPath,
    );
    return {
      path: formatted,
      label: parsed?.title?.trim() || getFilename(canonical) || fallbackLabel,
    };
  } catch {
    return { path: strippedPath, label: fallbackLabel };
  }
}

/**
 * Convert bytes to base64 in chunks to avoid stack overflow.
 */
export function bytesToBase64(bytes: Uint8Array): string {
  const chunkSize = 8192;
  let binary = '';
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length));
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

/**
 * Convert HEIC/HEIF blob to JPEG for browser display.
 * Delegates to the plugin-provided image converter service.
 * Returns original blob if no converter plugin is installed or conversion fails.
 */
export async function convertHeicToJpeg(blob: Blob): Promise<Blob> {
  const { convertBlobImage } = await import('./imageConverterService');
  return convertBlobImage(blob, '.heic', 'jpeg', 92);
}

/**
 * Normalize an attachment reference through the Rust link parser.
 *
 * Returns both:
 * - `canonical`: workspace-relative canonical path
 * - `sourceRelative`: path relative to `sourceEntryPath` for filesystem reads
 */
async function normalizeAttachmentReference(
  api: Api,
  sourceEntryPath: string,
  rawPath: string,
): Promise<{ canonical: string; sourceRelative: string }> {
  const cacheKey = `${sourceEntryPath}::${rawPath}`;
  const cached = normalizationCache.get(cacheKey);
  if (cached) return cached;

  const trimmed = rawPath.trim();
  const candidates = [trimmed];
  if (trimmed.startsWith('[') && trimmed.includes('](') && !trimmed.endsWith(')')) {
    candidates.push(`${trimmed})`);
  }

  for (const candidate of candidates) {
    try {
      const canonical = await api.canonicalizeLink(candidate, sourceEntryPath);
      const sourceRelative = await api.formatLink(
        canonical,
        getFilename(canonical) || 'attachment',
        'plain_relative',
        sourceEntryPath,
      );
      const result = { canonical, sourceRelative };
      normalizationCache.set(cacheKey, result);
      return result;
    } catch {
      // Try next candidate.
    }
  }

  const fallback = {
    canonical: trimmed,
    sourceRelative: trimmed,
  };
  normalizationCache.set(cacheKey, fallback);
  return fallback;
}

function unwrapAngleBracketPath(rawPath: string): string {
  const trimmed = rawPath.trim();
  if (trimmed.startsWith('<') && trimmed.endsWith('>')) {
    return trimmed.slice(1, -1).trim();
  }
  return trimmed;
}

function isNestedMarkdownLinkPayload(path: string): boolean {
  return path.startsWith('[') && path.includes('](');
}

function getAttachmentAssetPathHint(path: string): string {
  const trimmed = unwrapAngleBracketPath(path);
  if (!trimmed.endsWith('.md')) return trimmed;

  const withoutMarkdownSuffix = trimmed.slice(0, -3);
  const filename = withoutMarkdownSuffix.split('/').pop() ?? '';

  // Attachment notes preserve the original asset filename and append `.md`
  // (for example `_attachments/sample.html.md`). Strip only that wrapping
  // note suffix so preview blobs use the underlying asset MIME type.
  if (!filename.includes('.')) return trimmed;

  return withoutMarkdownSuffix;
}

const HTML_ATTACHMENT_PREVIEW_BRIDGE = String.raw`<script data-diaryx-html-preview-bridge>
(() => {
  const root = document.documentElement;
  let resizeObserver = null;
  let frame = 0;

  const postSize = () => {
    frame = 0;
    const doc = document.documentElement;
    const body = document.body;
    const height = Math.max(
      doc ? doc.scrollHeight : 0,
      doc ? doc.offsetHeight : 0,
      doc ? doc.clientHeight : 0,
      body ? body.scrollHeight : 0,
      body ? body.offsetHeight : 0,
      body ? body.clientHeight : 0,
    );
    if (!height || !window.parent) return;
    window.parent.postMessage({ type: "diaryx-html-attachment-size", height }, "*");
  };

  const schedulePostSize = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(postSize);
    });
  };

  const applyTheme = (message) => {
    if (!message || typeof message !== "object") return;
    if (message.theme === "dark" || message.theme === "light") {
      root.setAttribute("data-theme", message.theme);
    }

    const cssVars = message.cssVars;
    if (!cssVars || typeof cssVars !== "object") return;
    for (const [name, value] of Object.entries(cssVars)) {
      if (typeof value === "string" && name.startsWith("--")) {
        root.style.setProperty(name, value);
      }
    }
  };

  window.addEventListener("message", (event) => {
    const data = event.data;
    if (!data || typeof data !== "object") return;
    if (data.type === "init" || data.type === "theme-update") {
      applyTheme(data);
      schedulePostSize();
    }
  });

  window.addEventListener("load", schedulePostSize);
  window.addEventListener("resize", schedulePostSize);

  if (document.fonts && typeof document.fonts.ready?.then === "function") {
    document.fonts.ready.then(schedulePostSize).catch(() => {});
  }

  if (typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver(() => {
      schedulePostSize();
    });
    if (document.documentElement) resizeObserver.observe(document.documentElement);
    if (document.body) resizeObserver.observe(document.body);
  }

  window.addEventListener("beforeunload", () => {
    resizeObserver?.disconnect?.();
  }, { once: true });

  schedulePostSize();
})();
<\/script>`;

function injectHtmlAttachmentPreviewBridge(html: string): string {
  if (html.includes('data-diaryx-html-preview-bridge')) {
    return html;
  }

  if (/<\/body>/i.test(html)) {
    return html.replace(/<\/body>/i, `${HTML_ATTACHMENT_PREVIEW_BRIDGE}</body>`);
  }

  if (/<\/head>/i.test(html)) {
    return html.replace(/<\/head>/i, `${HTML_ATTACHMENT_PREVIEW_BRIDGE}</head>`);
  }

  return `${HTML_ATTACHMENT_PREVIEW_BRIDGE}${html}`;
}

function createHtmlPreviewBlobFromBytes(bytes: Uint8Array, mimeType: string): Blob {
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  const html = decoder.decode(bytes);
  const bridgedHtml = injectHtmlAttachmentPreviewBridge(html);
  return new Blob([encoder.encode(bridgedHtml)], { type: mimeType });
}

async function injectHtmlAttachmentPreviewBridgeIntoBlob(
  blob: Blob,
  mimeType: string,
): Promise<Blob> {
  const html = await blob.text();
  return new Blob([injectHtmlAttachmentPreviewBridge(html)], { type: mimeType });
}

async function resolveAttachmentFromPaths(
  api: Api,
  entryPath: string,
  canonicalPath: string,
  sourceRelativePath: string,
  signal: AbortSignal,
  logFailure: boolean,
): Promise<string | null> {
  // Reuse existing blob URL if we already resolved this attachment
  const existingBlobUrl = blobUrlMap.get(sourceRelativePath);
  if (existingBlobUrl) return existingBlobUrl;

  /**
   * Create a blob URL from raw bytes, handling HEIC conversion.
   */
  async function createBlobUrlFromBytes(bytes: Uint8Array): Promise<string | null> {
    if (signal.aborted) return null;
    const assetPathHint = getAttachmentAssetPathHint(canonicalPath);
    const mimeType = getMimeType(assetPathHint);
    let blob = isHtmlFile(assetPathHint)
      ? createHtmlPreviewBlobFromBytes(bytes, mimeType)
      : new Blob([bytes as unknown as BlobPart], { type: mimeType });
    if (isHeicFile(assetPathHint)) {
      blob = await convertHeicToJpeg(blob);
      if (signal.aborted) return null;
    }
    const blobUrl = URL.createObjectURL(blob);
    blobUrlMap.set(sourceRelativePath, blobUrl);
    return blobUrl;
  }

  /**
   * Try fetching from the server. Returns blob URL or null.
   */
  async function tryServerFetch(): Promise<string | null> {
    if (signal.aborted) return null;
    const serverUrl =
      getServerAttachmentUrl(entryPath, sourceRelativePath) ||
      (canonicalPath !== sourceRelativePath
        ? getServerAttachmentUrl(entryPath, canonicalPath)
        : null);
    if (!serverUrl) return null;
    try {
      const resp = await fetch(serverUrl, { signal });
      if (!resp.ok) return null;
      const assetPathHint = getAttachmentAssetPathHint(canonicalPath);
      const mimeType = getMimeType(assetPathHint);
      let blob = await resp.blob();
      if (signal.aborted) return null;
      if (isHtmlFile(assetPathHint)) {
        blob = await injectHtmlAttachmentPreviewBridgeIntoBlob(blob, mimeType);
      } else if (blob.type !== mimeType) {
        blob = new Blob([blob], { type: mimeType });
      }
      if (isHeicFile(assetPathHint)) {
        blob = await convertHeicToJpeg(blob);
        if (signal.aborted) return null;
      }
      const blobUrl = URL.createObjectURL(blob);
      blobUrlMap.set(sourceRelativePath, blobUrl);
      return blobUrl;
    } catch {
      return null;
    }
  }

  try {
    const storagePath = await api.resolveAttachmentStoragePath(entryPath, sourceRelativePath);
    if (signal.aborted) return null;
    const bytes = await api.readBinary(storagePath);
    if (signal.aborted) return null;

    // Check if local file hash matches CRDT metadata — if not, the attachment
    // was updated on another device and we should re-fetch from the server.
    const verification = await verifyLocalAttachmentAgainstMetadata(
      api,
      entryPath,
      canonicalPath,
      sourceRelativePath,
      storagePath,
      bytes,
      signal,
    );
    if (verification === 'aborted') return null;
    if (verification === 'stale') {
      const serverBlobUrl = await tryServerFetch();
      if (serverBlobUrl) return serverBlobUrl;
    }

    return await createBlobUrlFromBytes(bytes);
  } catch (e) {
    if (signal.aborted) return null;
    // Attachment not found locally — try streaming from server.
    const serverBlobUrl = await tryServerFetch();
    if (serverBlobUrl) return serverBlobUrl;

    if (logFailure) {
      console.warn(`[AttachmentService] Could not load attachment: ${sourceRelativePath}`, e);
    }
    return null;
  }
}

async function readAttachmentBytes(
  api: Api,
  sourceEntryPath: string,
  attachmentPath: string,
): Promise<Uint8Array> {
  const storagePath = await api.resolveAttachmentStoragePath(sourceEntryPath, attachmentPath);
  return api.readBinary(storagePath);
}

async function fetchAttachmentBlobFromServer(
  sourceEntryPath: string,
  attachmentPath: string,
): Promise<Blob | null> {
  const serverUrl = getServerAttachmentUrl(sourceEntryPath, attachmentPath);
  if (!serverUrl) return null;
  try {
    const response = await fetch(serverUrl);
    if (!response.ok) return null;
    let blob = await response.blob();
    const assetPathHint = getAttachmentAssetPathHint(attachmentPath);
    const mimeType = getMimeType(assetPathHint);
    if (isHtmlFile(assetPathHint)) {
      blob = await injectHtmlAttachmentPreviewBridgeIntoBlob(blob, mimeType);
    } else if (blob.type !== mimeType) {
      blob = new Blob([blob], { type: mimeType });
    }
    if (isHeicFile(assetPathHint)) {
      blob = await convertHeicToJpeg(blob);
    }
    return blob;
  } catch {
    return null;
  }
}

async function createThumbnailBlob(blob: Blob): Promise<Blob> {
  if (
    typeof document === 'undefined' ||
    !blob.type.startsWith('image/') ||
    blob.type === 'image/svg+xml'
  ) {
    return blob;
  }

  const previewUrl = URL.createObjectURL(blob);
  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = () => reject(new Error('Failed to decode image'));
      img.src = previewUrl;
    });

    const largestDimension = Math.max(image.naturalWidth, image.naturalHeight);
    if (!Number.isFinite(largestDimension) || largestDimension <= THUMBNAIL_MAX_DIMENSION_PX) {
      return blob;
    }

    const scale = THUMBNAIL_MAX_DIMENSION_PX / largestDimension;
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));

    const context = canvas.getContext('2d');
    if (!context) return blob;

    context.drawImage(image, 0, 0, canvas.width, canvas.height);

    const thumbnailBlob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob(resolve, 'image/jpeg', 0.82);
    });

    return thumbnailBlob ?? blob;
  } catch {
    return blob;
  } finally {
    URL.revokeObjectURL(previewUrl);
  }
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Revoke all tracked blob URLs (cleanup).
 * Should be called when switching documents or unmounting.
 */
export function revokeBlobUrls(): void {
  // Abort in-flight resolutions so they bail at the next check point.
  resolveAbort.abort();
  resolveAbort = new AbortController();

  // Flush the pending queue — these never touched the worker.
  for (const p of pendingResolves) p.cancel();
  pendingResolves.length = 0;

  for (const url of blobUrlMap.values()) {
    URL.revokeObjectURL(url);
  }
  blobUrlMap.clear();
  normalizationCache.clear();
}

/**
 * Clear cached thumbnail object URLs.
 * Useful for tests or explicit memory cleanup.
 */
export function clearAttachmentThumbnailCache(): void {
  for (const url of thumbnailUrlMap.values()) {
    URL.revokeObjectURL(url);
  }
  thumbnailUrlMap.clear();
  thumbnailPromiseMap.clear();
}

/**
 * Clear the positive local-attachment verification cache.
 * Useful for tests or explicit memory cleanup.
 */
export function clearAttachmentVerificationCache(): void {
  verifiedAttachmentMap.clear();
}

/**
 * Resolve a single attachment reference to a blob URL.
 * Returns null if the attachment can't be loaded.
 */
export async function resolveAttachment(
  api: Api,
  entryPath: string,
  rawPath: string,
): Promise<string | null> {
  // Capture the current abort signal so we can bail early if the user
  // navigates away (revokeBlobUrls aborts this signal).
  const signal = resolveAbort.signal;
  const directPath = unwrapAngleBracketPath(rawPath);
  const shouldTryDirectPath =
    directPath.length > 0 &&
    !directPath.startsWith('http://') &&
    !directPath.startsWith('https://') &&
    !directPath.startsWith('blob:') &&
    !isNestedMarkdownLinkPayload(directPath);

  // Fast path: most attachment refs are already plain relative paths. Try direct
  // lookup first to avoid extra link-parser calls on the backend command lane.
  if (shouldTryDirectPath) {
    const directResult = await resolveAttachmentFromPaths(
      api,
      entryPath,
      directPath,
      directPath,
      signal,
      false,
    );
    if (directResult || signal.aborted) return directResult;
  }

  const { canonical: canonicalPath, sourceRelative: sourceRelativePath } =
    await normalizeAttachmentReference(api, entryPath, rawPath);
  if (signal.aborted) return null;

  // Avoid repeating the same failed lookup when normalization resolves to the
  // same direct path we already attempted.
  if (
    shouldTryDirectPath &&
    canonicalPath === directPath &&
    sourceRelativePath === directPath
  ) {
    return null;
  }

  return resolveAttachmentFromPaths(
    api,
    entryPath,
    canonicalPath,
    sourceRelativePath,
    signal,
    true,
  );
}

/**
 * Queue an attachment resolution with concurrency control.
 * At most RESOLVE_CONCURRENCY resolutions hit the WASM worker at once;
 * the rest wait in a JS queue that is flushed instantly on entry switch.
 */
export function queueResolveAttachment(
  api: Api,
  entryPath: string,
  rawPath: string,
): Promise<string | null> {
  const signal = resolveAbort.signal;
  return new Promise<string | null>((resolve) => {
    const start = async () => {
      try {
        if (signal.aborted) { resolve(null); return; }
        resolve(await resolveAttachment(api, entryPath, rawPath));
      } catch {
        resolve(null);
      } finally {
        resolveInFlight--;
        drainPendingResolves();
      }
    };

    if (resolveInFlight < RESOLVE_CONCURRENCY) {
      resolveInFlight++;
      start();
    } else {
      pendingResolves.push({ start, cancel: () => resolve(null) });
    }
  });
}

/**
 * Transform attachment paths in markdown content to blob URLs for display.
 * Resolves all attachments in parallel for fast loading.
 *
 * @param content - Markdown content with attachment paths
 * @param entryPath - Path to the current entry (for resolving relative paths)
 * @param api - Api instance for reading attachment data
 * @returns Content with attachment paths replaced by blob URLs
 */
export async function transformAttachmentPaths(
  content: string,
  entryPath: string,
  api: Api | null,
): Promise<string> {
  if (!api) return content;

  // 1. Collect all matches synchronously
  const mediaRegex = /!\[([^\]]*)\]\((?:<([^>]+)>|([^)]+))\)/g;
  const matches: { fullMatch: string; alt: string; rawPath: string }[] = [];
  let match;

  while ((match = mediaRegex.exec(content)) !== null) {
    const rawPath = (match[2] || match[3]).trim();

    // Skip external URLs and already-transformed blob URLs
    if (
      rawPath.startsWith('http://') ||
      rawPath.startsWith('https://') ||
      rawPath.startsWith('blob:')
    ) {
      continue;
    }

    matches.push({ fullMatch: match[0], alt: match[1], rawPath });
  }

  if (matches.length === 0) return content;

  // 2. Resolve all attachments in parallel
  const results = await Promise.allSettled(
    matches.map(async (m) => {
      const blobUrl = await resolveAttachment(api, entryPath, m.rawPath);
      return { ...m, blobUrl };
    }),
  );

  // 3. Apply successful replacements
  let result = content;
  for (const r of results) {
    if (r.status === 'fulfilled' && r.value.blobUrl) {
      const { fullMatch, alt, blobUrl } = r.value;
      result = result.replace(fullMatch, `![${alt}](${blobUrl})`);
    }
  }

  return result;
}

/**
 * Reverse-transform blob URLs back to attachment paths (for saving).
 * Wraps paths with spaces in angle brackets for CommonMark compatibility.
 *
 * @param content - Markdown content with blob URLs
 * @returns Content with blob URLs replaced by original attachment paths
 */
export function reverseBlobUrlsToAttachmentPaths(content: string): string {
  let result = content;

  // Iterate through blobUrlMap (originalPath -> blobUrl) and replace blob URLs with original paths
  for (const [originalPath, blobUrl] of blobUrlMap.entries()) {
    const pathToUse = formatMarkdownDestination(originalPath);
    // Replace all occurrences of the blob URL with the original path
    result = result.replaceAll(blobUrl, pathToUse);
  }

  return result;
}

/**
 * Get the blob URL for an attachment path (if tracked).
 */
export function getBlobUrl(originalPath: string): string | undefined {
  return blobUrlMap.get(originalPath);
}

/**
 * Reverse-lookup: get the original attachment path for a blob URL.
 */
export function getPathForBlobUrl(blobUrl: string): string | undefined {
  for (const [path, url] of blobUrlMap.entries()) {
    if (url === blobUrl) return path;
  }
  return undefined;
}

/**
 * Track a blob URL for an attachment path.
 * Use this when creating blob URLs externally (e.g., for file uploads).
 */
export function trackBlobUrl(originalPath: string, blobUrl: string): void {
  blobUrlMap.set(originalPath, blobUrl);
}

/**
 * Check if we have any tracked blob URLs.
 */
export function hasBlobUrls(): boolean {
  return blobUrlMap.size > 0;
}

/**
 * Check whether an attachment exists locally without reading its bytes.
 */
export async function attachmentExistsLocally(
  api: Api,
  sourceEntryPath: string,
  attachmentPath: string,
): Promise<boolean> {
  try {
    const storagePath = await api.resolveAttachmentStoragePath(sourceEntryPath, attachmentPath);
    return await api.fileExists(storagePath);
  } catch {
    return false;
  }
}

/**
 * Resolve whether an attachment is available locally, only remotely, or unknown.
 */
export async function getAttachmentAvailability(
  api: Api,
  sourceEntryPath: string,
  attachmentPath: string,
): Promise<'local' | 'remote' | 'unknown'> {
  if (await attachmentExistsLocally(api, sourceEntryPath, attachmentPath)) {
    return 'local';
  }
  return getAttachmentMetadata(sourceEntryPath, attachmentPath) ? 'remote' : 'unknown';
}

/**
 * Load a cached downscaled thumbnail for an image attachment.
 * Falls back to the original blob when thumbnail generation is unsupported.
 */
export async function getAttachmentThumbnailUrl(
  api: Api,
  sourceEntryPath: string,
  attachmentPath: string,
): Promise<string | undefined> {
  const assetPathHint = getAttachmentAssetPathHint(attachmentPath);
  if (!isImageFile(assetPathHint)) return undefined;

  const cacheKey = getThumbnailCacheKey(sourceEntryPath, attachmentPath);
  const cachedUrl = touchThumbnailCacheEntry(cacheKey);
  if (cachedUrl) return cachedUrl;

  const inFlight = thumbnailPromiseMap.get(cacheKey);
  if (inFlight) return inFlight;

  const promise = (async () => {
    try {
      let blob: Blob | null = null;

      try {
        const bytes = await readAttachmentBytes(api, sourceEntryPath, attachmentPath);
        blob = new Blob([bytes as unknown as BlobPart], { type: getMimeType(assetPathHint) });
        if (isHeicFile(assetPathHint)) {
          blob = await convertHeicToJpeg(blob);
        }
      } catch {
        blob = await fetchAttachmentBlobFromServer(sourceEntryPath, attachmentPath);
      }

      if (!blob) return undefined;

      const thumbnailBlob = await createThumbnailBlob(blob);
      const url = URL.createObjectURL(thumbnailBlob);
      thumbnailUrlMap.set(cacheKey, url);
      evictThumbnailCacheIfNeeded();
      return url;
    } catch {
      return undefined;
    } finally {
      thumbnailPromiseMap.delete(cacheKey);
    }
  })();

  thumbnailPromiseMap.set(cacheKey, promise);
  return promise;
}

/**
 * Peek an already-cached thumbnail URL without triggering thumbnail generation.
 */
export function getCachedAttachmentThumbnailUrl(
  sourceEntryPath: string,
  attachmentPath: string,
): string | undefined {
  return touchThumbnailCacheEntry(getThumbnailCacheKey(sourceEntryPath, attachmentPath));
}

/**
 * Resolve a preview-friendly full image URL.
 *
 * On Tauri, this prefers a native asset URL for local verified files and falls
 * back to the shared blob resolver otherwise.
 */
export async function resolvePreviewMediaSrc(
  api: Api,
  entryPath: string,
  rawPath: string,
): Promise<string | undefined> {
  const kind = getAttachmentMediaKind(rawPath);
  if (!isPreviewableAttachmentKind(kind)) {
    return undefined;
  }

  if (!isTauri()) {
    const blobUrl = await resolveAttachment(api, entryPath, rawPath);
    return blobUrl ?? undefined;
  }

  const directPath = unwrapAngleBracketPath(rawPath);
  const shouldTryDirectPath =
    directPath.length > 0 &&
    !directPath.startsWith('http://') &&
    !directPath.startsWith('https://') &&
    !directPath.startsWith('blob:') &&
    !isNestedMarkdownLinkPayload(directPath);

  if (shouldTryDirectPath) {
    const nativeUrl = await resolveTauriNativePreviewUrlFromPaths(
      api,
      entryPath,
      directPath,
      directPath,
    );
    if (nativeUrl) return nativeUrl;
  }

  const { canonical: canonicalPath, sourceRelative: sourceRelativePath } =
    await normalizeAttachmentReference(api, entryPath, rawPath);

  if (
    !shouldTryDirectPath ||
    canonicalPath !== directPath ||
    sourceRelativePath !== directPath
  ) {
    const nativeUrl = await resolveTauriNativePreviewUrlFromPaths(
      api,
      entryPath,
      canonicalPath,
      sourceRelativePath,
    );
    if (nativeUrl) return nativeUrl;
  }

  const blobUrl = await resolveAttachment(api, entryPath, rawPath);
  return blobUrl ?? undefined;
}

/**
 * Backward-compatible image preview helper.
 */
export async function resolvePreviewImageSrc(
  api: Api,
  entryPath: string,
  rawPath: string,
): Promise<string | undefined> {
  return resolvePreviewMediaSrc(api, entryPath, rawPath);
}

/**
 * Resolve a local image path to a blob URL for display.
 * Returns the blob URL on success, or undefined if the attachment can't be loaded.
 * Delegates to resolveAttachment for all local path resolution.
 */
export async function resolveImageSrc(
  rawImagePath: string,
  entryPath: string,
  api: Api,
): Promise<string | undefined> {
  // Skip URLs that don't need resolution
  if (
    rawImagePath.startsWith('http://') ||
    rawImagePath.startsWith('https://') ||
    rawImagePath.startsWith('blob:') ||
    rawImagePath.startsWith('data:')
  ) {
    return rawImagePath;
  }

  const blobUrl = await resolveAttachment(api, entryPath, rawImagePath);
  return blobUrl ?? undefined;
}

// ============================================================================
// Path Utilities
// ============================================================================

/**
 * Get the directory portion of a path (browser-compatible dirname).
 */
function getDirectory(filePath: string): string {
  const lastSlash = filePath.lastIndexOf('/');
  return lastSlash >= 0 ? filePath.substring(0, lastSlash) : '';
}

/**
 * Join path segments (browser-compatible path.join).
 */
function joinPaths(...segments: string[]): string {
  return segments
    .filter(s => s.length > 0)
    .join('/')
    .replace(/\/+/g, '/'); // Remove duplicate slashes
}

/**
 * Compute a relative path from one directory to another (browser-compatible).
 */
function relativePath(fromDir: string, toDir: string): string {
  if (fromDir === toDir) return '';

  const fromParts = fromDir.split('/').filter(p => p.length > 0);
  const toParts = toDir.split('/').filter(p => p.length > 0);

  // Find common prefix
  let commonLength = 0;
  while (
    commonLength < fromParts.length &&
    commonLength < toParts.length &&
    fromParts[commonLength] === toParts[commonLength]
  ) {
    commonLength++;
  }

  // Build relative path: go up from 'from', then down to 'to'
  const upCount = fromParts.length - commonLength;
  const ups = Array(upCount).fill('..');
  const downs = toParts.slice(commonLength);

  return [...ups, ...downs].join('/');
}

/**
 * Compute the relative path from the current entry to an attachment
 * that may be defined in an ancestor entry.
 *
 * @param currentEntryPath - Path to the current entry (e.g., "2025/01/day.md")
 * @param sourceEntryPath - Path to entry containing the attachment (e.g., "2025/01.index.md")
 * @param attachmentPath - The attachment path relative to source (e.g., "header.png")
 * @returns Relative path from current entry to attachment
 */
export function computeRelativeAttachmentPath(
  currentEntryPath: string,
  sourceEntryPath: string,
  attachmentPath: string
): string {
  // If same entry, just return attachment path
  if (currentEntryPath === sourceEntryPath) {
    return attachmentPath;
  }

  // Get directories
  const currentDir = getDirectory(currentEntryPath);
  const sourceDir = getDirectory(sourceEntryPath);

  // Compute relative path from current dir to source dir
  const relToSource = relativePath(currentDir, sourceDir);

  // Join with attachment path
  return relToSource ? joinPaths(relToSource, attachmentPath) : attachmentPath;
}
