/**
 * Background download queue for non-markdown workspace files (attachments,
 * images, PDFs, etc.) that were deferred during initial workspace download.
 *
 * Downloads files via the namespace objects API (`GET /namespaces/{nsId}/objects/{key}`)
 * and writes them directly to the workspace filesystem. The queue persists in
 * localStorage and resumes across app restarts.
 */

import type { Api } from "$lib/backend/api";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const QUEUE_STORAGE_KEY = "diaryx_deferred_file_queue_v1";
const CONCURRENCY = 8;
const MAX_ATTEMPTS = 5;
const BASE_BACKOFF_MS = 2_000;
const MAX_BACKOFF_MS = 60_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type ItemState = "pending" | "downloading" | "complete" | "failed";

interface QueueItem {
  id: string;
  nsId: string;
  /** Server-side object key, e.g. `files/_attachments/photo.png`. */
  objectKey: string;
  /** Workspace-relative path to write to, e.g. `_attachments/photo.png`. */
  workspacePath: string;
  state: ItemState;
  attempts: number;
  nextAttemptAt: number;
  lastError?: string;
}

export interface DeferredQueueProgress {
  total: number;
  completed: number;
  inProgress: number;
  failed: number;
}

type ProgressListener = (progress: DeferredQueueProgress) => void;

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let queue: QueueItem[] = loadQueue();
let api: Api | null = null;
let serverUrl: string | null = null;
let authToken: string | null = null;
let inFlight = 0;
let pumpScheduled = false;
let progressListeners: ProgressListener[] = [];

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

function loadQueue(): QueueItem[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(QUEUE_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as QueueItem[];
    return Array.isArray(parsed) ? parsed.filter((i) => i?.id && i?.objectKey) : [];
  } catch {
    return [];
  }
}

function persistQueue(): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(queue));
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

function computeProgress(): DeferredQueueProgress {
  let completed = 0;
  let inProgress = 0;
  let failed = 0;
  for (const item of queue) {
    if (item.state === "complete") completed++;
    else if (item.state === "downloading") inProgress++;
    else if (item.state === "failed") failed++;
  }
  return { total: queue.length, completed, inProgress, failed };
}

function emitProgress(): void {
  if (progressListeners.length === 0) return;
  const p = computeProgress();
  for (const listener of progressListeners) {
    try { listener(p); } catch { /* ignore */ }
  }
}

/** Subscribe to progress updates. Returns an unsubscribe function. */
export function onDeferredQueueProgress(listener: ProgressListener): () => void {
  progressListeners.push(listener);
  // Emit current state immediately.
  try { listener(computeProgress()); } catch { /* ignore */ }
  return () => {
    progressListeners = progressListeners.filter((l) => l !== listener);
  };
}

/** Get a snapshot of current queue progress. */
export function getDeferredQueueProgress(): DeferredQueueProgress {
  return computeProgress();
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/**
 * Initialize (or re-initialize) the deferred download queue with a backend
 * and auth credentials. Call this on app startup and after workspace download.
 * Automatically starts pumping any pending items.
 */
export function initDeferredQueue(
  backend: Api,
  server: string,
  token: string,
): void {
  api = backend;
  serverUrl = server.replace(/\/$/, "");
  authToken = token;
  ensureOnlineHooks();
  schedulePump();
}

// ---------------------------------------------------------------------------
// Enqueueing
// ---------------------------------------------------------------------------

/**
 * Bulk-enqueue deferred file keys returned by the sync engine.
 * Keys look like `files/_attachments/photo.png`; the `files/` prefix is
 * stripped to derive the workspace-relative write path.
 */
export function enqueueDeferredFiles(
  nsId: string,
  keys: string[],
): void {
  if (keys.length === 0) return;

  // Build a set of existing keys to avoid duplicates.
  const existing = new Set(queue.map((i) => `${i.nsId}:${i.objectKey}`));
  let added = 0;

  for (const key of keys) {
    const dedupKey = `${nsId}:${key}`;
    if (existing.has(dedupKey)) continue;

    const workspacePath = key.startsWith("files/") ? key.slice(6) : key;
    queue.push({
      id: crypto.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`,
      nsId,
      objectKey: key,
      workspacePath,
      state: "pending",
      attempts: 0,
      nextAttemptAt: 0,
    });
    existing.add(dedupKey);
    added++;
  }

  if (added > 0) {
    persistQueue();
    emitProgress();
    schedulePump();
  }
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/**
 * Move the item matching `objectKey` to the front of the queue so it
 * downloads next. Returns `true` if the item was found and moved.
 */
export function prioritizeFile(objectKey: string): boolean {
  const index = queue.findIndex(
    (i) => i.objectKey === objectKey && i.state !== "complete" && i.state !== "downloading",
  );
  if (index <= 0) return index === 0;

  const [item] = queue.splice(index, 1);
  item.nextAttemptAt = 0; // eligible immediately
  if (item.state === "failed") item.state = "pending";
  queue.unshift(item);
  persistQueue();
  schedulePump();
  return true;
}

// ---------------------------------------------------------------------------
// Download processing
// ---------------------------------------------------------------------------

async function processItem(item: QueueItem): Promise<void> {
  if (!api || !serverUrl || !authToken) return;

  item.state = "downloading";
  item.attempts++;
  item.lastError = undefined;
  persistQueue();
  emitProgress();

  try {
    // Encode key segments for the URL path.
    const encodedKey = item.objectKey
      .split("/")
      .map(encodeURIComponent)
      .join("/");
    const url = `${serverUrl}/namespaces/${encodeURIComponent(item.nsId)}/objects/${encodedKey}`;
    const response = await fetch(url, {
      method: "GET",
      credentials: "include",
      headers: authToken ? { Authorization: `Bearer ${authToken}` } : {},
    });

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const bytes = new Uint8Array(await response.arrayBuffer());
    await api.writeBinary(item.workspacePath, bytes);

    item.state = "complete";
  } catch (e) {
    item.lastError = e instanceof Error ? e.message : String(e);

    if (item.attempts >= MAX_ATTEMPTS) {
      item.state = "failed";
    } else {
      // Exponential backoff.
      const delay = Math.min(BASE_BACKOFF_MS * 2 ** (item.attempts - 1), MAX_BACKOFF_MS);
      item.nextAttemptAt = Date.now() + delay;
      item.state = "pending";
    }
  } finally {
    inFlight--;
    persistQueue();
    emitProgress();
    schedulePump();
  }
}

// ---------------------------------------------------------------------------
// Queue pump
// ---------------------------------------------------------------------------

function schedulePump(): void {
  if (pumpScheduled) return;
  pumpScheduled = true;
  queueMicrotask(async () => {
    pumpScheduled = false;
    await pump();
  });
}

async function pump(): Promise<void> {
  if (!api || !serverUrl || !authToken) return;
  if (typeof navigator !== "undefined" && navigator.onLine === false) return;

  const now = Date.now();
  for (const item of queue) {
    if (inFlight >= CONCURRENCY) break;
    if (item.state !== "pending") continue;
    if (item.nextAttemptAt > now) continue;

    inFlight++;
    void processItem(item);
  }
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

/** Remove completed items from the queue to free localStorage space. */
export function pruneCompleted(): void {
  const before = queue.length;
  queue = queue.filter((i) => i.state !== "complete");
  if (queue.length !== before) {
    persistQueue();
    emitProgress();
  }
}

// ---------------------------------------------------------------------------
// Online/offline hooks
// ---------------------------------------------------------------------------

function ensureOnlineHooks(): void {
  if (typeof window === "undefined") return;
  const flag = "__diaryxDeferredQueueHooksInstalled";
  const w = window as unknown as Record<string, unknown>;
  if (w[flag]) return;
  w[flag] = true;
  window.addEventListener("online", () => schedulePump());
}
