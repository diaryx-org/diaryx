/**
 * Attachment Service
 *
 * Manages blob URLs for displaying attachments in the editor.
 * Handles transforming file paths to blob URLs and back.
 */

import type { Api } from '$lib/backend/api';
import heic2any from 'heic2any';
import { getServerAttachmentUrl, getAttachmentMetadata, sha256Hex } from './attachmentSyncService';

// ============================================================================
// State
// ============================================================================

// Blob URL tracking for attachments (originalPath -> blobUrl)
const blobUrlMap = new Map<string, string>();

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
 * Check if a file is a displayable media file (image, video, or audio).
 */
export function isMediaFile(path: string): boolean {
  return isImageFile(path) || isVideoFile(path) || isAudioFile(path);
}

/**
 * Extract the filename from a path.
 */
export function getFilename(path: string): string {
  return path.split('/').pop() || path;
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
 * Returns original blob if conversion fails.
 */
export async function convertHeicToJpeg(blob: Blob): Promise<Blob> {
  try {
    const result = await heic2any({
      blob,
      toType: 'image/jpeg',
      quality: 0.92,
    });
    // heic2any can return an array of blobs for multi-image HEIC files
    return Array.isArray(result) ? result[0] : result;
  } catch (e) {
    console.warn('[AttachmentService] HEIC conversion failed:', e);
    return blob;
  }
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
    const mimeType = getMimeType(canonicalPath);
    let blob = new Blob([bytes as unknown as BlobPart], { type: mimeType });
    if (isHeicFile(canonicalPath)) {
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
      const mimeType = getMimeType(canonicalPath);
      let blob = await resp.blob();
      if (signal.aborted) return null;
      if (blob.type !== mimeType) {
        blob = new Blob([blob], { type: mimeType });
      }
      if (isHeicFile(canonicalPath)) {
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
    const data = await api.getAttachmentData(entryPath, sourceRelativePath);
    if (signal.aborted) return null;
    const bytes = new Uint8Array(data);

    // Check if local file hash matches CRDT metadata — if not, the attachment
    // was updated on another device and we should re-fetch from the server.
    const meta =
      getAttachmentMetadata(entryPath, sourceRelativePath) ||
      (canonicalPath !== sourceRelativePath
        ? getAttachmentMetadata(entryPath, canonicalPath)
        : null);
    if (meta) {
      const localHash = await sha256Hex(bytes);
      if (signal.aborted) return null;
      if (localHash !== meta.hash) {
        const serverBlobUrl = await tryServerFetch();
        if (serverBlobUrl) return serverBlobUrl;
      }
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
      pendingResolves.push({ start: () => { resolveInFlight++; start(); }, cancel: () => resolve(null) });
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
    // Wrap path in angle brackets if it contains spaces (CommonMark spec)
    const pathToUse = originalPath.includes(' ')
      ? `<${originalPath}>`
      : originalPath;
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
