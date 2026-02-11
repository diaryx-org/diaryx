import { createApi, type Api } from "$lib/backend/api";
import type { Backend } from "$lib/backend/interface";
import type { BinaryRef } from "$lib/backend/generated";
import { AuthError, createAuthService } from "$lib/auth/authService";

const QUEUE_STORAGE_KEY = "diaryx_attachment_sync_queue_v1";
const PART_SIZE_BYTES = 8 * 1024 * 1024;
const MAX_PART_ATTEMPTS = 5;
const UPLOAD_CONCURRENCY = 2;
const DOWNLOAD_CONCURRENCY = 2;
const STORAGE_USAGE_REFRESH_DEBOUNCE_MS = 1500;

type QueueItemState = "pending" | "uploading" | "downloading" | "complete" | "failed";
type QueueKind = "upload" | "download";

interface BaseQueueItem {
  id: string;
  kind: QueueKind;
  state: QueueItemState;
  workspaceId: string;
  entryPath: string;
  attachmentPath: string;
  hash: string;
  mimeType: string;
  sizeBytes: number;
  attempts: number;
  nextAttemptAt: number;
  createdAt: number;
  updatedAt: number;
  lastError?: string;
}

interface UploadQueueItem extends BaseQueueItem {
  kind: "upload";
}

interface DownloadQueueItem extends BaseQueueItem {
  kind: "download";
}

type QueueItem = UploadQueueItem | DownloadQueueItem;

interface SyncContext {
  enabled: boolean;
  serverUrl: string | null;
  authToken: string | null;
  workspaceId: string | null;
}

interface UploadJobInput {
  workspaceId: string;
  entryPath: string;
  attachmentPath: string;
  hash: string;
  mimeType: string;
  sizeBytes: number;
}

interface DownloadJobInput {
  workspaceId: string;
  entryPath: string;
  attachmentPath: string;
  hash: string;
  mimeType: string;
  sizeBytes: number;
}

interface AttachmentLookup {
  hash: string;
  mimeType: string;
  sizeBytes: number;
  workspaceId: string;
}

const attachmentIndex = new Map<string, AttachmentLookup>();
let queue: QueueItem[] = loadQueue();
let syncContext: SyncContext = {
  enabled: false,
  serverUrl: null,
  authToken: null,
  workspaceId: null,
};
let backendApi: Api | null = null;
let queuePumpScheduled = false;
let uploadInFlight = 0;
let downloadInFlight = 0;
let storageUsageRefreshTimer: ReturnType<typeof setTimeout> | null = null;
let storageUsageRefreshInFlight = false;

function loadQueue(): QueueItem[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(QUEUE_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as QueueItem[];
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item) => item && item.id && item.kind);
  } catch {
    return [];
  }
}

function persistQueue(): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(QUEUE_STORAGE_KEY, JSON.stringify(queue));
}

function nowMs(): number {
  return Date.now();
}

function queueKey(kind: QueueKind, workspaceId: string, entryPath: string, attachmentPath: string, hash: string): string {
  return `${kind}:${workspaceId}:${entryPath}:${attachmentPath}:${hash}`;
}

function itemKey(item: QueueItem): string {
  return queueKey(item.kind, item.workspaceId, item.entryPath, item.attachmentPath, item.hash);
}

function isReadyToRun(): boolean {
  return !!(
    syncContext.enabled &&
    syncContext.serverUrl &&
    syncContext.authToken &&
    syncContext.workspaceId &&
    backendApi &&
    (typeof navigator === "undefined" || navigator.onLine !== false)
  );
}

function ensureOnlineHooks(): void {
  if (typeof window === "undefined") return;
  const flag = "__diaryxAttachmentSyncHooksInstalled";
  const globalObj = window as unknown as Record<string, unknown>;
  if (globalObj[flag]) return;
  globalObj[flag] = true;
  window.addEventListener("online", () => scheduleQueuePump());
  window.addEventListener("offline", () => {
    // No-op: jobs remain pending and will resume when online.
  });
}

function scheduleQueuePump(): void {
  if (queuePumpScheduled) return;
  queuePumpScheduled = true;
  queueMicrotask(async () => {
    queuePumpScheduled = false;
    await pumpQueue();
  });
}

function scheduleStorageUsageRefresh(): void {
  if (typeof window === "undefined") return;
  if (storageUsageRefreshTimer !== null) return;
  storageUsageRefreshTimer = setTimeout(() => {
    storageUsageRefreshTimer = null;
    void refreshStorageUsage();
  }, STORAGE_USAGE_REFRESH_DEBOUNCE_MS);
}

async function refreshStorageUsage(): Promise<void> {
  if (storageUsageRefreshInFlight) return;
  storageUsageRefreshInFlight = true;
  try {
    const authStore = await import("$lib/auth/authStore.svelte");
    await authStore.refreshUserStorageUsage();
  } catch (error) {
    console.warn("[AttachmentSyncService] Failed to refresh synced storage usage:", error);
  } finally {
    storageUsageRefreshInFlight = false;
  }
}

function backoffDelayMs(attempt: number): number {
  const base = Math.min(30000, 500 * 2 ** Math.max(0, attempt - 1));
  const jitter = Math.floor(Math.random() * 250);
  return base + jitter;
}

function updateItem(id: string, mutator: (item: QueueItem) => QueueItem): void {
  const idx = queue.findIndex((item) => item.id === id);
  if (idx < 0) return;
  queue[idx] = mutator(queue[idx]);
  persistQueue();
}

function removeCompletedItems(): void {
  const before = queue.length;
  queue = queue.filter((item) => item.state !== "complete");
  if (queue.length !== before) {
    persistQueue();
  }
}

function resolveAttachmentStoragePath(entryPath: string, attachmentPath: string): string {
  const entryDir = entryPath.includes("/") ? entryPath.slice(0, entryPath.lastIndexOf("/")) : "";
  const raw = attachmentPath.startsWith("/")
    ? attachmentPath.slice(1)
    : [entryDir, attachmentPath].filter(Boolean).join("/");
  const segments: string[] = [];
  for (const segment of raw.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") {
      if (segments.length > 0) segments.pop();
      continue;
    }
    segments.push(segment);
  }
  return segments.join("/");
}

async function uploadPartWithRetry(
  authToken: string,
  serverUrl: string,
  workspaceId: string,
  uploadId: string,
  partNo: number,
  payload: Uint8Array,
): Promise<void> {
  const auth = createAuthService(serverUrl);
  const body = Uint8Array.from(payload).buffer as ArrayBuffer;
  let attempt = 0;
  while (attempt < MAX_PART_ATTEMPTS) {
    try {
      await auth.uploadAttachmentPart(authToken, workspaceId, uploadId, partNo, body);
      return;
    } catch (error) {
      attempt += 1;
      if (attempt >= MAX_PART_ATTEMPTS) {
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, backoffDelayMs(attempt)));
    }
  }
}

async function processUpload(item: UploadQueueItem): Promise<void> {
  if (!backendApi || !syncContext.serverUrl || !syncContext.authToken) {
    throw new Error("Attachment sync upload is not configured");
  }
  const auth = createAuthService(syncContext.serverUrl);
  const bytes = new Uint8Array(
    await backendApi.getAttachmentData(item.entryPath, item.attachmentPath),
  );
  const totalParts = Math.max(1, Math.ceil(bytes.byteLength / PART_SIZE_BYTES));
  const init = await auth.initAttachmentUpload(syncContext.authToken, item.workspaceId, {
    attachment_path: item.attachmentPath,
    hash: item.hash,
    size_bytes: item.sizeBytes,
    mime_type: item.mimeType,
    part_size: PART_SIZE_BYTES,
    total_parts: totalParts,
  });

  if (init.status === "already_exists" || !init.upload_id) {
    return;
  }

  const uploaded = new Set(init.uploaded_parts);
  for (let partNo = 1; partNo <= totalParts; partNo++) {
    if (uploaded.has(partNo)) continue;
    const start = (partNo - 1) * PART_SIZE_BYTES;
    const end = Math.min(bytes.byteLength, start + PART_SIZE_BYTES);
    const payload = bytes.slice(start, end);
    await uploadPartWithRetry(
      syncContext.authToken,
      syncContext.serverUrl,
      item.workspaceId,
      init.upload_id,
      partNo,
      payload,
    );
  }

  let complete = await auth.completeAttachmentUpload(syncContext.authToken, item.workspaceId, init.upload_id, {
    attachment_path: item.attachmentPath,
    hash: item.hash,
    size_bytes: item.sizeBytes,
    mime_type: item.mimeType,
  });

  if (!complete.ok && complete.missing_parts && complete.missing_parts.length > 0) {
    for (const partNo of complete.missing_parts) {
      const start = (partNo - 1) * PART_SIZE_BYTES;
      const end = Math.min(bytes.byteLength, start + PART_SIZE_BYTES);
      const payload = bytes.slice(start, end);
      await uploadPartWithRetry(
        syncContext.authToken,
        syncContext.serverUrl,
        item.workspaceId,
        init.upload_id,
        partNo,
        payload,
      );
    }

    complete = await auth.completeAttachmentUpload(syncContext.authToken, item.workspaceId, init.upload_id, {
      attachment_path: item.attachmentPath,
      hash: item.hash,
      size_bytes: item.sizeBytes,
      mime_type: item.mimeType,
    });
  }

  if (!complete.ok) {
    throw new Error("Attachment upload completion failed");
  }
}

async function processDownload(item: DownloadQueueItem): Promise<void> {
  if (!backendApi || !syncContext.serverUrl || !syncContext.authToken) {
    throw new Error("Attachment sync download is not configured");
  }
  const auth = createAuthService(syncContext.serverUrl);
  const response = await auth.downloadAttachment(
    syncContext.authToken,
    item.workspaceId,
    item.hash,
  );
  const storagePath = resolveAttachmentStoragePath(item.entryPath, item.attachmentPath);
  await backendApi.writeBinary(storagePath, response.bytes);
}

function applyFailure(item: QueueItem, error: unknown): void {
  const isQuotaFailure = isTerminalAttachmentSyncError(error);
  const nextAttempts = item.attempts + 1;
  const exhausted = isQuotaFailure || nextAttempts >= MAX_PART_ATTEMPTS;
  const updated: QueueItem = {
    ...item,
    attempts: nextAttempts,
    state: exhausted ? "failed" : "pending",
    nextAttemptAt: exhausted ? nowMs() : nowMs() + backoffDelayMs(nextAttempts),
    updatedAt: nowMs(),
    lastError: isQuotaFailure
      ? `Quota exceeded: ${
          error instanceof Error ? error.message : "Storage limit exceeded"
        }`
      : error instanceof Error
        ? error.message
        : String(error),
  };
  updateItem(item.id, () => updated);
  if (isQuotaFailure) {
    scheduleStorageUsageRefresh();
  }
}

export function isTerminalAttachmentSyncError(error: unknown): boolean {
  if (error instanceof AuthError) {
    return error.statusCode === 413;
  }
  if (error && typeof error === "object") {
    const statusCode = (error as { statusCode?: unknown }).statusCode;
    return typeof statusCode === "number" && statusCode === 413;
  }
  return false;
}

async function runQueueItem(item: QueueItem): Promise<void> {
  const processingState: QueueItemState = item.kind === "upload" ? "uploading" : "downloading";
  updateItem(item.id, (existing) => ({
    ...existing,
    state: processingState,
    updatedAt: nowMs(),
  }));

  try {
    if (item.kind === "upload") {
      await processUpload(item);
    } else {
      await processDownload(item);
    }
    updateItem(item.id, (existing) => ({
      ...existing,
      state: "complete",
      attempts: 0,
      updatedAt: nowMs(),
      lastError: undefined,
    }));
    if (item.kind === "upload") {
      scheduleStorageUsageRefresh();
    }
  } catch (error) {
    applyFailure(item, error);
  } finally {
    removeCompletedItems();
    scheduleQueuePump();
  }
}

async function pumpQueue(): Promise<void> {
  if (!isReadyToRun()) return;

  const now = nowMs();
  const uploadSlots = Math.max(0, UPLOAD_CONCURRENCY - uploadInFlight);
  const downloadSlots = Math.max(0, DOWNLOAD_CONCURRENCY - downloadInFlight);
  if (uploadSlots === 0 && downloadSlots === 0) return;

  const pendingUploads = queue
    .filter(
      (item): item is UploadQueueItem =>
        item.kind === "upload" &&
        (item.state === "pending" || item.state === "failed") &&
        item.nextAttemptAt <= now,
    )
    .slice(0, uploadSlots);
  const pendingDownloads = queue
    .filter(
      (item): item is DownloadQueueItem =>
        item.kind === "download" &&
        (item.state === "pending" || item.state === "failed") &&
        item.nextAttemptAt <= now,
    )
    .slice(0, downloadSlots);

  for (const upload of pendingUploads) {
    uploadInFlight += 1;
    runQueueItem(upload).finally(() => {
      uploadInFlight = Math.max(0, uploadInFlight - 1);
    });
  }
  for (const download of pendingDownloads) {
    downloadInFlight += 1;
    runQueueItem(download).finally(() => {
      downloadInFlight = Math.max(0, downloadInFlight - 1);
    });
  }
}

function upsertQueueItem(nextItem: QueueItem): void {
  const key = itemKey(nextItem);
  const existingIndex = queue.findIndex((item) => itemKey(item) === key);
  if (existingIndex >= 0) {
    const existing = queue[existingIndex];
    queue[existingIndex] = {
      ...existing,
      ...nextItem,
      id: existing.id,
      state: existing.state === "uploading" || existing.state === "downloading" ? existing.state : "pending",
      attempts: existing.attempts,
      nextAttemptAt: nowMs(),
      updatedAt: nowMs(),
    };
  } else {
    queue.push(nextItem);
  }
  persistQueue();
  scheduleQueuePump();
}

function metadataIndexKey(entryPath: string, attachmentPath: string): string {
  return `${entryPath}::${attachmentPath}`;
}

function stripMetadataIndexForEntry(entryPath: string): void {
  for (const key of attachmentIndex.keys()) {
    if (key.startsWith(`${entryPath}::`)) {
      attachmentIndex.delete(key);
    }
  }
}

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const normalized = Uint8Array.from(bytes);
  const digest = await crypto.subtle.digest("SHA-256", normalized);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

export function setAttachmentSyncBackend(backend: Backend | null): void {
  backendApi = backend ? createApi(backend) : null;
  ensureOnlineHooks();
  scheduleQueuePump();
}

export function setAttachmentSyncContext(context: Partial<SyncContext>): void {
  syncContext = {
    ...syncContext,
    ...context,
  };
  scheduleQueuePump();
}

export function getAttachmentSyncQueueSnapshot(): QueueItem[] {
  return [...queue];
}

export function indexAttachmentRefs(entryPath: string, attachments: BinaryRef[], workspaceId: string): void {
  stripMetadataIndexForEntry(entryPath);
  for (const ref of attachments) {
    if (!ref.hash || ref.deleted) continue;
    attachmentIndex.set(metadataIndexKey(entryPath, ref.path), {
      hash: ref.hash,
      mimeType: ref.mime_type,
      sizeBytes: Number(ref.size ?? 0n),
      workspaceId,
    });
  }
}

export function enqueueAttachmentUpload(job: UploadJobInput): void {
  const ts = nowMs();
  upsertQueueItem({
    id: crypto.randomUUID(),
    kind: "upload",
    state: "pending",
    workspaceId: job.workspaceId,
    entryPath: job.entryPath,
    attachmentPath: job.attachmentPath,
    hash: job.hash,
    mimeType: job.mimeType,
    sizeBytes: job.sizeBytes,
    attempts: 0,
    nextAttemptAt: ts,
    createdAt: ts,
    updatedAt: ts,
  });
}

export function enqueueAttachmentDownload(job: DownloadJobInput): void {
  const ts = nowMs();
  upsertQueueItem({
    id: crypto.randomUUID(),
    kind: "download",
    state: "pending",
    workspaceId: job.workspaceId,
    entryPath: job.entryPath,
    attachmentPath: job.attachmentPath,
    hash: job.hash,
    mimeType: job.mimeType,
    sizeBytes: job.sizeBytes,
    attempts: 0,
    nextAttemptAt: ts,
    createdAt: ts,
    updatedAt: ts,
  });
}

export function enqueueMissingDownloadsFromMetadata(
  entryPath: string,
  workspaceId: string,
  attachments: BinaryRef[],
): void {
  if (!backendApi) return;
  indexAttachmentRefs(entryPath, attachments, workspaceId);
  for (const ref of attachments) {
    if (!ref.hash || ref.deleted) continue;
    void backendApi
      .getAttachmentData(entryPath, ref.path)
      .catch(() => {
        enqueueAttachmentDownload({
          workspaceId,
          entryPath,
          attachmentPath: ref.path,
          hash: ref.hash,
          mimeType: ref.mime_type || "application/octet-stream",
          sizeBytes: Number(ref.size ?? 0n),
        });
      });
  }
}

export function requestMissingBlobDownload(entryPath: string, attachmentPath: string): boolean {
  const metadata = attachmentIndex.get(metadataIndexKey(entryPath, attachmentPath));
  if (!metadata) return false;
  enqueueAttachmentDownload({
    workspaceId: metadata.workspaceId,
    entryPath,
    attachmentPath,
    hash: metadata.hash,
    mimeType: metadata.mimeType,
    sizeBytes: metadata.sizeBytes,
  });
  return true;
}

export function retryFailedAttachmentJobs(): void {
  const ts = nowMs();
  queue = queue.map((item) =>
    item.state === "failed"
      ? {
          ...item,
          state: "pending",
          attempts: 0,
          nextAttemptAt: ts,
          updatedAt: ts,
          lastError: undefined,
        }
      : item,
  );
  persistQueue();
  scheduleQueuePump();
}

export type {
  SyncContext,
  QueueItem,
  UploadJobInput,
  DownloadJobInput,
};
