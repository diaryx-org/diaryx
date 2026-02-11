/**
 * Workspace CRDT Bridge - replaces workspaceCrdt.ts with Rust CRDT backend.
 *
 * This module provides the same API surface as the original workspaceCrdt.ts
 * but delegates all operations to the Rust CRDT via RustCrdtApi.
 *
 * ## Doc-ID Based Architecture
 *
 * Files are keyed by stable document IDs (UUIDs) rather than file paths.
 * This makes renames trivial property updates rather than delete+create operations.
 *
 * Key changes:
 * - Body sync subscriptions are keyed by doc_id (stable across renames)
 * - Use `getPathForDocId()` to derive filesystem paths
 * - Use `findDocIdByPath()` to look up doc_ids from paths
 * - The `filename` field in FileMetadata contains the filename on disk
 *
 * Supports Hocuspocus server-based sync for device-to-device synchronization.
 */

import { RustCrdtApi } from './rustCrdtApi';
import { UnifiedSyncTransport, createUnifiedSyncTransport } from './unifiedSyncTransport';
import type { FileMetadata, BinaryRef } from '../backend/generated';
import type { Backend, FileSystemEvent, SyncEvent } from '../backend/interface';
import { crdt_update_file_index, isStorageReady } from '$lib/storage/sqliteStorageBridge.js';
import type { Api } from '../backend/api';
import { shareSessionStore } from '@/models/stores/shareSessionStore.svelte';
import { collaborationStore } from '@/models/stores/collaborationStore.svelte';
import { getToken } from '$lib/auth/authStore.svelte';
import * as syncHelpers from './syncHelpers';
import {
  setAttachmentSyncBackend,
  setAttachmentSyncContext,
  enqueueMissingDownloadsFromMetadata,
} from '@/models/services/attachmentSyncService';

/**
 * Convert an HTTP URL to a WebSocket URL for sync v2 (/sync2).
 */
function toWebSocketUrl(httpUrl: string): string {
  return httpUrl
    .replace(/^https:\/\//, 'wss://')
    .replace(/^http:\/\//, 'ws://')
    .replace(/\/sync2?$/, '')
    .replace(/\/$/, '');
}

/**
 * Check if a path refers to a temporary file that should not be synced.
 */
function isTempFile(path: string): boolean {
  return path.endsWith('.tmp') || path.endsWith('.bak') || path.endsWith('.swap');
}

function extractLinkPath(raw: string): string | null {
  const value = raw.trim();
  if (!value) return null;

  if (!value.startsWith('[')) {
    return value;
  }

  const closeBracket = value.indexOf(']');
  if (closeBracket <= 0 || value.slice(closeBracket, closeBracket + 2) !== '](') {
    return value;
  }

  const rest = value.slice(closeBracket + 2);
  if (!rest) return null;

  // Angle-bracket form: [Title](<path with spaces>)
  if (rest.startsWith('<')) {
    const closeAngle = rest.indexOf('>');
    if (closeAngle <= 1 || rest.slice(closeAngle + 1, closeAngle + 2) !== ')') {
      return null;
    }
    return rest.slice(1, closeAngle).trim() || null;
  }

  // Standard markdown URL form with balanced parentheses support.
  let depth = 0;
  for (let i = 0; i < rest.length; i++) {
    const ch = rest[i];
    if (ch === '(') {
      depth++;
    } else if (ch === ')') {
      if (depth === 0) {
        return rest.slice(0, i).trim() || null;
      }
      depth--;
    }
  }

  return null;
}

function normalizeWorkspacePath(path: string): string {
  const normalized: string[] = [];
  for (const segment of path.split('/')) {
    if (!segment || segment === '.') continue;
    if (segment === '..') {
      if (normalized.length > 0) normalized.pop();
      continue;
    }
    normalized.push(segment);
  }
  return normalized.join('/');
}

/**
 * Resolve a CRDT metadata reference to canonical workspace-relative path.
 *
 * - Supports markdown links (`[Title](path)` and `[Title](<path>)`)
 * - Supports workspace-root paths (`/path/file.md`)
 * - Supports explicit relative paths (`./`, `../`)
 * - For ambiguous plain paths, defaults to workspace-root and can disambiguate
 *   against known paths when provided.
 */
export function resolveCrdtReferencePath(
  currentFilePath: string,
  rawReference: string | null | undefined,
  knownPaths?: Set<string>
): string | null {
  if (!rawReference) return null;
  const extracted = extractLinkPath(rawReference);
  if (!extracted) return null;

  let reference = extracted.trim();
  if (!reference) return null;

  if (reference.startsWith('<') && reference.endsWith('>')) {
    reference = reference.slice(1, -1).trim();
  }

  if (!reference) return null;

  if (reference.startsWith('/')) {
    return normalizeWorkspacePath(reference.slice(1)) || null;
  }

  const baseDir = (() => {
    const idx = currentFilePath.lastIndexOf('/');
    return idx >= 0 ? currentFilePath.slice(0, idx) : '';
  })();

  const isExplicitRelative =
    reference.startsWith('./') ||
    reference.startsWith('../') ||
    reference === '.' ||
    reference === '..';

  if (isExplicitRelative) {
    const combined = baseDir ? `${baseDir}/${reference}` : reference;
    return normalizeWorkspacePath(combined) || null;
  }

  // Ambiguous path: treat as workspace-root canonical by default.
  const workspaceRootCandidate = normalizeWorkspacePath(reference);
  if (!knownPaths) {
    return workspaceRootCandidate || null;
  }

  const relativeCandidate = normalizeWorkspacePath(
    baseDir ? `${baseDir}/${reference}` : reference
  );

  if (relativeCandidate && relativeCandidate !== workspaceRootCandidate) {
    const rootExists = knownPaths.has(workspaceRootCandidate);
    const relativeExists = knownPaths.has(relativeCandidate);

    if (rootExists && !relativeExists) return workspaceRootCandidate;
    if (relativeExists && !rootExists) return relativeCandidate;
  }

  return workspaceRootCandidate || null;
}

function eventTouchesTempFile(event: FileSystemEvent): boolean {
  switch (event.type) {
    case 'FileCreated':
    case 'FileDeleted':
    case 'MetadataChanged':
    case 'ContentsChanged':
      return isTempFile(event.path);
    case 'FileRenamed':
      return isTempFile(event.old_path) || isTempFile(event.new_path);
    case 'FileMoved': {
      const oldPath = event.old_parent && event.old_parent.length > 0
        ? `${event.old_parent}/${event.path.split('/').pop() ?? ''}`
        : event.path;
      return isTempFile(event.path) || isTempFile(oldPath);
    }
    default:
      return false;
  }
}

// Cross-module singleton support.
// Vite dev server may create separate module instances when the same file is
// imported via different paths (e.g., "$lib/crdt/workspaceCrdtBridge" from app
// code vs "/src/lib/crdt/workspaceCrdtBridge" from page.evaluate in tests).
// We register key functions on globalThis so unconfigured module instances
// can delegate to the configured one.
const _g = globalThis as any;
if (!_g.__diaryx_bridge) {
  _g.__diaryx_bridge = {} as Record<string, Function>;
}

/** Register this module instance as the configured bridge on globalThis. */
function registerBridgeOnGlobal(): void {
  _g.__diaryx_bridge.ensureBodySync = _ensureBodySyncImpl;
  _g.__diaryx_bridge.getBodyContentFromCrdt = _getBodyContentFromCrdtImpl;
  _g.__diaryx_bridge.getFileMetadata = _getFileMetadataImpl;
}

// State
let rustApi: RustCrdtApi | null = null;
let _originalRustApi: RustCrdtApi | null = null; // Saved before guest override, restored on session end
let _originalServerUrl: string | null = null; // Saved before session override, restored on session end
let _backend: Backend | null = null;

// Native sync state (Tauri only)
let _nativeSyncActive = false;
let _nativeSyncUnsubscribe: (() => void) | null = null;

let serverUrl: string | null = null;
let _workspaceId: string | null = null;
let initialized = false;
let _initializing = false;

// Initial sync tracking - allows waiting for first sync to complete
let _initialSyncComplete = false;
let _initialSyncResolvers: Array<() => void> = [];

// Unified sync v2 transport (single WebSocket for workspace + body)
let unifiedSyncTransport: UnifiedSyncTransport | null = null;

// Outgoing local updates captured before the transport is available.
// This can happen during reconnects or sync mode transitions.
let pendingLocalSyncUpdates: Array<{ docId: string; bytes: Uint8Array }> = [];

// Cached server URL (WebSocket) for sync readiness checks
let _serverUrl: string | null = null;

// Flag: true when this client loaded from server (load_server mode).
// When set, body CRDTs are cleared before sync to prevent duplication
// (importFromZip populates body CRDT locally, then server sends the same content).
let _freshFromServerLoad = false;

export function setFreshFromServerLoad(value: boolean): void {
  _freshFromServerLoad = value;
}

export function isFreshFromServerLoad(): boolean {
  return _freshFromServerLoad;
}

function refreshAttachmentSyncContext(): void {
  setAttachmentSyncContext({
    enabled: Boolean(serverUrl && _workspaceId && getToken()),
    serverUrl,
    authToken: getToken(),
    workspaceId: _workspaceId,
  });
}

/**
 * Reset local body CRDT docs before a "load from server" sync connection.
 *
 * Without this, previously loaded local body docs (for example, default README.md
 * content) can contribute independent Yjs operations that merge with server
 * operations, causing duplicated body content on initial join.
 */
async function resetBodyDocsForFreshServerLoad(): Promise<void> {
  if (!_freshFromServerLoad || !rustApi) return;

  try {
    const loadedDocs = await rustApi.listLoadedBodyDocs();
    const activeFiles = await rustApi.listFiles(false);
    const docNames = new Set<string>(loadedDocs);
    for (const [path] of activeFiles) {
      docNames.add(path);
    }

    if (docNames.size === 0) return;

    console.log(`[WorkspaceCrdtBridge] Fresh server load: resetting ${docNames.size} body doc(s)`);
    for (const docName of docNames) {
      if (!docName || isTempFile(docName)) continue;
      try {
        await rustApi.resetBodyDoc(docName);
      } catch (error) {
        console.warn(`[WorkspaceCrdtBridge] Failed to reset body doc '${docName}' before sync:`, error);
      }
    }
  } catch (error) {
    console.warn('[WorkspaceCrdtBridge] Failed to prepare fresh server body state:', error);
  }
}

// Per-file mutex to prevent race conditions on concurrent updates
// Map of path -> Promise that resolves when the lock is released
const fileLocks = new Map<string, Promise<void>>();

// Track pending intervals/timeouts for proper cleanup
const pendingIntervals: Set<ReturnType<typeof setInterval>> = new Set();
const pendingTimeouts: Set<ReturnType<typeof setTimeout>> = new Set();

async function flushPendingLocalSyncUpdates(): Promise<void> {
  if (pendingLocalSyncUpdates.length === 0) {
    return;
  }

  // In "load from server" mode we intentionally discard any queued local sync
  // updates captured during bootstrap. Replaying them would merge stale local
  // operations (for example, default README content) into server state.
  if (_freshFromServerLoad) {
    const dropped = pendingLocalSyncUpdates.length;
    pendingLocalSyncUpdates = [];
    console.log(
      `[WorkspaceCrdtBridge] Fresh server load: dropped ${dropped} queued local sync update(s)`
    );
    return;
  }

  if (!unifiedSyncTransport) {
    return;
  }

  const pending = pendingLocalSyncUpdates;
  pendingLocalSyncUpdates = [];
  console.log(`[WorkspaceCrdtBridge] Flushing ${pending.length} queued local sync update(s)`);

  for (const { docId, bytes } of pending) {
    try {
      await unifiedSyncTransport.queueLocalUpdate(docId, bytes);
    } catch (err) {
      console.warn('[WorkspaceCrdtBridge] Failed to flush queued sync update, re-queueing:', err);
      pendingLocalSyncUpdates.unshift({ docId, bytes });
      break;
    }
  }
}

/**
 * Discard queued local sync updates that were captured while no transport was connected.
 *
 * This is used by bootstrap flows (snapshot load/upload) where replaying pre-connect
 * local updates would duplicate or conflict with snapshot state.
 *
 * Returns the number of dropped updates.
 */
export function discardQueuedLocalSyncUpdates(reason: string = 'manual'): number {
  const dropped = pendingLocalSyncUpdates.length;
  if (dropped > 0) {
    console.log(`[WorkspaceCrdtBridge] Discarding ${dropped} queued local sync update(s): ${reason}`);
    pendingLocalSyncUpdates = [];
  }
  return dropped;
}

/**
 * Acquire a lock for a specific file path.
 * Returns a release function to call when done.
 * This prevents concurrent read-modify-write races on the same file.
 */
async function acquireFileLock(path: string): Promise<() => void> {
  // Wait for any existing lock to be released
  while (fileLocks.has(path)) {
    await fileLocks.get(path);
  }

  // Create a new lock
  let releaseLock: () => void;
  const lockPromise = new Promise<void>((resolve) => {
    releaseLock = resolve;
  });
  fileLocks.set(path, lockPromise);

  // Return the release function
  return () => {
    fileLocks.delete(path);
    releaseLock!();
  };
}

// Callbacks
type FileChangeCallback = (path: string | null, metadata: FileMetadata | null) => void;
const fileChangeCallbacks = new Set<FileChangeCallback>();

// File rename callbacks - called when a file path changes
type FileRenamedCallback = (oldPath: string, newPath: string) => void;
const fileRenamedCallbacks = new Set<FileRenamedCallback>();

// Session sync callbacks - called when session data is received and synced
type SessionSyncCallback = () => void;
const sessionSyncCallbacks = new Set<SessionSyncCallback>();

// Body change callbacks - called when a file's body content changes remotely
type BodyChangeCallback = (path: string, body: string) => void;
const bodyChangeCallbacks = new Set<BodyChangeCallback>();

// Sync progress callbacks - called to report sync progress
type SyncProgressCallback = (completed: number, total: number) => void;
const syncProgressCallbacks = new Set<SyncProgressCallback>();

// Sync status callbacks - called when sync status changes
type SyncStatusCallback = (status: 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', error?: string) => void;
const syncStatusCallbacks = new Set<SyncStatusCallback>();

// ===========================================================================
// Configuration
// ===========================================================================

/**
 * Set the server URL for workspace sync.
 * For Tauri: Uses native Rust sync client for better performance.
 * For WASM/web: Creates and connects a UnifiedSyncTransport.
 *
 * IMPORTANT: setBackend() must be called before this function.
 * If backend is null, sync operations will fail silently or throw.
 */
export async function setWorkspaceServer(url: string | null): Promise<void> {
  const previousUrl = serverUrl;
  serverUrl = url;
  _serverUrl = url ? toWebSocketUrl(url) : null;  // v2 WebSocket URL for readiness checks
  refreshAttachmentSyncContext();

  console.log('[WorkspaceCrdtBridge] setWorkspaceServer:', url ? 'connecting' : 'disconnecting');

  // Skip if URL hasn't changed or not initialized
  if (previousUrl === url || !initialized || !rustApi) {
    return;
  }

  // Validate backend is initialized before proceeding with sync setup
  if (url && !_backend) {
    console.error('[WorkspaceCrdtBridge] CRITICAL: setWorkspaceServer called with URL but _backend is null!');
    console.error('[WorkspaceCrdtBridge] Call setBackend() before setWorkspaceServer() to avoid silent failures.');
    // Don't throw to avoid breaking existing code, but log prominently
    notifySyncStatus('error', 'Sync initialization failed: backend not configured');
    return;
  }

  // Disconnect existing sync (native or browser-based)
  await disconnectExistingSync();

  // Create new sync if URL is set
  if (url && _backend) {
    // "Load from server" should start from empty body docs to avoid
    // merging local bootstrap operations into server content.
    if (_freshFromServerLoad) {
      await resetBodyDocsForFreshServerLoad();
    }

    // Reset initial sync tracking since we're starting a new sync connection
    _initialSyncComplete = false;
    _initialSyncResolvers = [];

    const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';

    // Check if backend supports native sync (Tauri)
    if (_backend.hasNativeSync?.() && _backend.startSync) {
      // Use _serverUrl (WebSocket URL) for native sync - Rust client expects wss:// scheme
      console.log('[WorkspaceCrdtBridge] Using native sync (Tauri)');

      // Set up event listener for native sync events
      if (_backend.onSyncEvent) {
        _nativeSyncUnsubscribe = _backend.onSyncEvent((event: SyncEvent) => {
          handleNativeSyncEvent(event);
        });
      }

      // Notify connecting status
      notifySyncStatus('connecting');

      try {
        _nativeSyncActive = true;
        await _backend.startSync(_serverUrl!, workspaceDocName, getToken() ?? undefined);
        console.log('[WorkspaceCrdtBridge] Native sync started successfully');
      } catch (e) {
        console.error('[WorkspaceCrdtBridge] Native sync failed to start:', e);
        notifySyncStatus('error', e instanceof Error ? e.message : String(e));
        _nativeSyncActive = false;
      }
    } else {
      // Use browser WebSocket (WASM/web)
      console.log('[WorkspaceCrdtBridge] Using browser sync (v2)');

      // Use UnifiedSyncTransport (single WebSocket for workspace + body via /sync2)
      {
        const v2Url = toWebSocketUrl(url);

        unifiedSyncTransport = createUnifiedSyncTransport({
          serverUrl: v2Url,
          workspaceId: _workspaceId!,
          backend: _backend,
          writeToDisk: true,
          authToken: getToken() ?? undefined,
          onStatusChange: (connected) => {
            notifySyncStatus(connected ? 'syncing' : 'idle');
            if (!connected) {
              collaborationStore.setBodySyncStatus('idle');
            }
          },
          onError: (message) => {
            notifySyncStatus('error', message);
          },
          onWorkspaceSynced: async () => {
            notifySyncStatus('synced');
            notifyFileChange(null, null);
            await updateFileIndexFromCrdt();
            markInitialSyncComplete();
            // Body sync is handled automatically by SyncSession in Rust
            _freshFromServerLoad = false;
            collaborationStore.setBodySyncStatus('synced');
          },
          onSyncComplete: (filesSynced) => {
            console.log(`[UnifiedSync] Sync complete: ${filesSynced} files synced`);
            collaborationStore.setBodySyncStatus('synced');
          },
          onFilesChanged: async () => {
            notifyFileChange(null, null);
          },
          onBodyChanged: (filePath) => {
            // Body content changed remotely — read from CRDT and notify UI
            if (rustApi) {
              rustApi.getBodyContent(filePath).then(content => {
                if (content) {
                  notifyBodyChange(filePath, content);
                }
              }).catch(e => {
                console.warn('[WorkspaceCrdtBridge] Failed to get body content:', e);
              });
            }
          },
          onProgress: (completed, total) => {
            notifySyncProgress(completed, total);
          },
        });

        // Notify connecting status
        notifySyncStatus('connecting');

        await unifiedSyncTransport.connect();
        await flushPendingLocalSyncUpdates();
      }
    }

    registerBridgeOnGlobal();
  }
}

/**
 * Handle native sync events from Tauri.
 */
function handleNativeSyncEvent(event: SyncEvent): void {

  switch (event.type) {
    case 'status-changed':
      // Map status to our internal status type
      // Track metadata and body status separately for accurate UI representation
      const metadataConnected = event.status.metadata === 'connected';
      const bodyConnected = event.status.body === 'connected';
      const isConnecting = event.status.metadata === 'connecting' || event.status.body === 'connecting';

      // Update body sync status based on native sync state
      if (bodyConnected) {
        collaborationStore.setBodySyncStatus('synced');
      } else if (event.status.body === 'connecting') {
        collaborationStore.setBodySyncStatus('syncing');
      } else {
        collaborationStore.setBodySyncStatus('idle');
      }

      // Update metadata sync status
      if (metadataConnected && bodyConnected) {
        notifySyncStatus('synced');
      } else if (isConnecting) {
        notifySyncStatus('connecting');
      } else if (metadataConnected) {
        // Metadata connected but body not yet - show syncing
        notifySyncStatus('syncing');
      } else {
        notifySyncStatus('idle');
      }
      break;

    case 'files-changed':
      console.log('[WorkspaceCrdtBridge] Native sync: files changed:', event.paths);
      // Mark initial sync complete when we receive first files-changed event
      if (!_initialSyncComplete) {
        markInitialSyncComplete();
        updateFileIndexFromCrdt();
        // Auto-sync body content in background
        autoSyncBodiesInBackground();
      }
      notifyFileChange(null, null);
      break;

    case 'body-changed':
      console.log('[WorkspaceCrdtBridge] Native sync: body changed:', event.path);
      // The body content is already in the CRDT, fetch it and notify
      if (rustApi) {
        rustApi.getBodyContent(event.path).then(content => {
          if (content) {
            notifyBodyChange(event.path, content);
          }
        }).catch(e => {
          console.warn('[WorkspaceCrdtBridge] Failed to get body content:', e);
        });
      }
      break;

    case 'progress':
      notifySyncProgress(event.completed, event.total);
      break;

    case 'error':
      console.error('[WorkspaceCrdtBridge] Native sync error:', event.message);
      notifySyncStatus('error', event.message);
      break;
  }
}

/**
 * Disconnect existing sync (native or browser-based).
 */
async function disconnectExistingSync(): Promise<void> {
  // Stop native sync if active
  if (_nativeSyncActive && _backend?.stopSync) {
    console.log('[WorkspaceCrdtBridge] Stopping native sync');
    try {
      await _backend.stopSync();
    } catch (e) {
      console.warn('[WorkspaceCrdtBridge] Error stopping native sync:', e);
    }
    _nativeSyncActive = false;
  }

  // Unsubscribe from native sync events
  if (_nativeSyncUnsubscribe) {
    _nativeSyncUnsubscribe();
    _nativeSyncUnsubscribe = null;
  }

  // Reset body sync status since we're disconnecting
  collaborationStore.resetBodySyncStatus();

  // Disconnect v2 unified sync transport if any
  if (unifiedSyncTransport) {
    console.log('[WorkspaceCrdtBridge] Disconnecting UnifiedSyncTransport');
    unifiedSyncTransport.destroy();
    unifiedSyncTransport = null;
  }
}

/**
 * Auto-sync body content for all files in background.
 * Body sync is now automatic via SyncSession, so this is a no-op.
 *
 * @deprecated Body sync is automatic via SyncSession.
 */
async function autoSyncBodiesInBackground(): Promise<void> {
  // Body sync is automatic via SyncSession in Rust.
}



/**
 * Get the current workspace server URL.
 */
export function getWorkspaceServer(): string | null {
  return serverUrl;
}

/**
 * Set the initializing state (for UI feedback).
 */
export function setInitializing(value: boolean): void {
  _initializing = value;
}

/**
 * Set the workspace ID for room naming.
 * If server URL is already set and ID changes, reconnect workspace sync.
 */
export async function setWorkspaceId(id: string | null): Promise<void> {
  console.log('[WorkspaceCrdtBridge] setWorkspaceId:', { id, previousId: _workspaceId });
  const previousId = _workspaceId;
  _workspaceId = id;
  refreshAttachmentSyncContext();

  if (id !== previousId) {
    discardQueuedLocalSyncUpdates('workspace id changed');
  }

  // If we have a server URL and the ID changed, reconnect with the new doc name
  if (serverUrl && id && id !== previousId) {
    console.log('[WorkspaceCrdtBridge] Workspace ID changed, reconnecting workspace sync');
    // Force reconnect by temporarily clearing serverUrl
    const savedUrl = serverUrl;
    serverUrl = null;
    await setWorkspaceServer(savedUrl);
  }
}

/**
 * Set the backend API for file operations.
 * This is used to write synced file content to disk for guests.
 */
/**
 * @deprecated Body content loading is now handled in Rust via SyncSession.
 * This function is kept for backward compatibility.
 */
export function setBackendApi(_api: Api): void {
  // No-op: body content loading is handled by SyncSession in Rust.
}

/**
 * Set the backend for sync operations.
 * This is used for Rust-backed sync helpers that need direct backend access.
 */
export function setBackend(backend: Backend): void {
  _backend = backend;
  setAttachmentSyncBackend(backend);
  refreshAttachmentSyncContext();
}

/**
 * Get the storage path for a file in guest mode.
 *
 * For guests using in-memory storage (web): Returns the original path (no prefix needed).
 * For guests using OPFS (Tauri, future): Prefixes with guest/{joinCode}/... to isolate storage.
 * For hosts: Returns the original path.
 */
function getGuestStoragePath(originalPath: string): string {
  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  const usesInMemory = shareSessionStore.usesInMemoryStorage;

  console.log('[WorkspaceCrdtBridge] getGuestStoragePath:', {
    originalPath,
    isGuest,
    joinCode,
    usesInMemory,
    mode: shareSessionStore.mode
  });

  // Hosts don't need path prefixing
  if (!isGuest || !joinCode) {
    return originalPath;
  }

  // Guests using in-memory storage don't need path prefixing
  // (they have their own isolated filesystem)
  if (usesInMemory) {
    console.log('[WorkspaceCrdtBridge] Using original path (in-memory storage):', originalPath);
    return originalPath;
  }

  // Guests using OPFS need path prefixing to isolate their storage
  const guestPath = `guest/${joinCode}/${originalPath}`;
  console.log('[WorkspaceCrdtBridge] Using guest path (OPFS):', guestPath);
  return guestPath;
}

function getCanonicalPathFallback(storagePath: string): string {
  let path = storagePath;

  // Strip leading "./" for consistent path format (server uses paths without ./ prefix)
  if (path.startsWith('./')) {
    path = path.slice(2);
  }

  const isGuest = shareSessionStore.isGuest;
  const joinCode = shareSessionStore.joinCode;
  const usesInMemory = shareSessionStore.usesInMemoryStorage;

  if (!isGuest || !joinCode) {
    return path;
  }

  // Guests using in-memory storage don't have prefixes
  if (usesInMemory) {
    return path;
  }

  // Strip guest/{joinCode}/ prefix if present (for OPFS guests)
  const guestPrefix = `guest/${joinCode}/`;
  if (path.startsWith(guestPrefix)) {
    return path.slice(guestPrefix.length);
  }

  return path;
}

/**
 * Convert a storage path to canonical form using backend sync-path rules.
 *
 * This is the preferred path normalizer for sync comparisons because it
 * delegates to Rust (`GetCanonicalPath`) so host/guest rules stay consistent
 * with the backend.
 *
 * Falls back to local canonicalization when backend is unavailable.
 */
export async function getCanonicalPathForSync(storagePath: string): Promise<string> {
  if (_backend) {
    try {
      return await syncHelpers.getCanonicalPath(_backend, storagePath);
    } catch (error) {
      console.warn('[WorkspaceCrdtBridge] Backend canonical path failed, using fallback:', error);
    }
  }
  return getCanonicalPathFallback(storagePath);
}

/**
 * Convert a guest storage path back to canonical path (fallback-only helper).
 *
 * Prefer `getCanonicalPathForSync()` for sync decisions.
 */
export function getCanonicalPath(storagePath: string): string {
  return getCanonicalPathFallback(storagePath);
}

// Session code for share sessions
let _sessionCode: string | null = null;

/**
 * Notify all body change callbacks.
 */
function notifyBodyChange(path: string, body: string): void {
  for (const callback of bodyChangeCallbacks) {
    try {
      callback(path, body);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] Body change callback error:', error);
    }
  }
}

/**
 * Callbacks for share session events delivered via the V2 sync transport.
 */
export interface SessionSyncCallbacks {
  onSessionJoined?: (data: { joinCode: string; workspaceId: string; readOnly: boolean }) => void;
  onPeerJoined?: (guestId: string, peerCount: number) => void;
  onPeerLeft?: (guestId: string, peerCount: number) => void;
  onSessionEnded?: () => void;
}

/**
 * Start syncing with a share session via V2 unified transport.
 * Uses a single WebSocket for workspace metadata + all body document sync.
 * @param sessionServerUrl - The sync server base URL
 * @param sessionCode - The join code for the session
 * @param isHost - If true, sends initial state to server
 * @param workspaceId - The actual workspace ID (required for guests; V2 doc IDs use this)
 * @param callbacks - Optional callbacks for session events (peer join/leave, session joined/ended)
 */
export async function startSessionSync(
  sessionServerUrl: string,
  sessionCode: string,
  isHost: boolean = false,
  workspaceId?: string,
  callbacks?: SessionSyncCallbacks,
): Promise<void> {
  console.log('[WorkspaceCrdtBridge] Starting session sync (V2):', sessionCode, 'isHost:', isHost);

  _sessionCode = sessionCode;

  // Set module-level server URL and workspace ID for body sync bridges.
  // Both host and guest need these so getOrCreateBodyBridge() can subscribe
  // body docs through the session transport. Without this, the guard at
  // `if (!_serverUrl || !_workspaceId) return` silently skips body subscriptions.
  _originalServerUrl = _serverUrl;
  _serverUrl = toWebSocketUrl(sessionServerUrl);
  if (!isHost) {
    // For guests, workspaceId must be the real workspace ID (looked up via REST before connecting).
    _workspaceId = workspaceId!;
  }
  console.log('[WorkspaceCrdtBridge] Session: set _serverUrl and _workspaceId for body sync:', {
    _serverUrl,
    _workspaceId,
    isHost,
  });

  // Disconnect existing transport
  if (unifiedSyncTransport) {
    unifiedSyncTransport.destroy();
    unifiedSyncTransport = null;
  }

  // For guests, recreate rustApi with the guest backend so CRDT commands
  // go through the in-memory guest filesystem, not the original backend.
  if (!isHost && _backend) {
    _originalRustApi = rustApi; // Save original for restoration in stopSessionSync
    rustApi = new RustCrdtApi(_backend);
    console.log('[WorkspaceCrdtBridge] Guest: recreated rustApi with guest backend');


  }

  if (!rustApi) {
    console.error('[WorkspaceCrdtBridge] RustCrdtApi not initialized for session');
    return;
  }

  // Configure sync handler for guests (path prefixing, etc.)
  if (!isHost && _backend) {
    const usesOpfs = !shareSessionStore.usesInMemoryStorage;
    console.log('[WorkspaceCrdtBridge] Configuring sync handler for guest:', { sessionCode, usesOpfs });
    await syncHelpers.configureSyncHandler(_backend, sessionCode, usesOpfs);
  }

  if (!_backend) {
    console.error('[WorkspaceCrdtBridge] Backend not initialized for session sync');
    return;
  }

  // Create a promise that resolves when initial workspace sync completes
  let syncResolve: () => void;
  const syncPromise = new Promise<void>((resolve) => {
    syncResolve = resolve;
  });

  console.log('[WorkspaceCrdtBridge] Creating UnifiedSyncTransport for session:', sessionServerUrl, 'session:', sessionCode);

  unifiedSyncTransport = createUnifiedSyncTransport({
    serverUrl: sessionServerUrl,
    workspaceId: _workspaceId!,
    backend: _backend,
    writeToDisk: true,
    authToken: getToken() ?? undefined,
    sessionCode: sessionCode,
    onStatusChange: (connected) => {
      console.log('[WorkspaceCrdtBridge] Session sync status:', connected);
      notifySyncStatus(connected ? 'syncing' : 'idle');
      if (!connected) {
        collaborationStore.setBodySyncStatus('idle');
      }
    },
    onError: (message) => {
      notifySyncStatus('error', message);
    },
    onWorkspaceSynced: async () => {
      console.log('[WorkspaceCrdtBridge] Session workspace sync complete, isHost:', isHost);
      notifySyncStatus('synced');
      if (!isHost) notifySessionSync();
      syncResolve();
      // Body sync is handled automatically by SyncSession in Rust
      _freshFromServerLoad = false;
      collaborationStore.setBodySyncStatus('synced');
    },
    onFilesChanged: async () => {
      shareSessionStore.isGuest ? notifySessionSync() : notifyFileChange(null, null);
    },
    onBodyChanged: (filePath) => {
      // Body content changed remotely — read from CRDT and notify UI
      if (rustApi) {
        rustApi.getBodyContent(filePath).then(content => {
          if (content) {
            notifyBodyChange(filePath, content);
          }
        }).catch(e => {
          console.warn('[WorkspaceCrdtBridge] Failed to get body content:', e);
        });
      }
    },
    onProgress: (completed, total) => {
      console.log('[WorkspaceCrdtBridge] Session sync progress:', completed, '/', total);
      notifySyncProgress(completed, total);
    },
    onSyncComplete: (filesSynced) => {
      console.log(`[SessionSync] Sync complete: ${filesSynced} files synced`);
      collaborationStore.setBodySyncStatus('synced');
    },
    // Pass through session callbacks to the transport
    onSessionJoined: callbacks?.onSessionJoined,
    onPeerJoined: callbacks?.onPeerJoined,
    onPeerLeft: callbacks?.onPeerLeft,
    onSessionEnded: callbacks?.onSessionEnded,
  });

  notifySyncStatus('connecting');
  await unifiedSyncTransport.connect();
  await flushPendingLocalSyncUpdates();

  // Wait for initial sync to complete (with timeout)
  const timeoutPromise = new Promise<void>((_, reject) => {
    setTimeout(() => reject(new Error('Session sync timeout')), 15000);
  });

  try {
    await Promise.race([syncPromise, timeoutPromise]);
    console.log('[WorkspaceCrdtBridge] Session sync fully complete');
  } catch (error) {
    console.warn('[WorkspaceCrdtBridge] Session sync did not complete in time, continuing anyway');
  }
}

/**
 * Stop syncing with a share session.
 */
export async function stopSessionSync(): Promise<void> {
  console.log('[WorkspaceCrdtBridge] Stopping session sync');

  _sessionCode = null;

  // Cancel any pending debounced session sync notification
  if (_sessionSyncTimer !== null) {
    clearTimeout(_sessionSyncTimer);
    _sessionSyncTimer = null;
  }
  _sessionSyncRunning = false;
  _sessionSyncPending = false;

  // Restore original server URL if it was overridden for the session
  if (_originalServerUrl !== null) {
    _serverUrl = _originalServerUrl;
    _originalServerUrl = null;
    console.log('[WorkspaceCrdtBridge] Restored original _serverUrl after session');
  }

  // Restore original rustApi if it was overridden for a guest session
  if (_originalRustApi) {
    rustApi = _originalRustApi;
    _originalRustApi = null;
    console.log('[WorkspaceCrdtBridge] Restored original rustApi after guest session');
  }

  // Clear sync handler guest configuration
  if (_backend) {
    await syncHelpers.configureSyncHandler(_backend, null, false);
  }

  // Destroy unified transport (handles both workspace + body sync)
  if (unifiedSyncTransport) {
    unifiedSyncTransport.destroy();
    unifiedSyncTransport = null;
  }

  collaborationStore.setBodySyncStatus('idle');
}

/**
 * Get the current session code.
 */
export function getSessionCode(): string | null {
  return _sessionCode;
}

/**
 * Get the current workspace ID.
 */
export function getWorkspaceId(): string | null {
  return _workspaceId;
}

/**
 * Check if device-to-device sync is active.
 * True when we have a unified sync transport connected but no live share session.
 */
export function isDeviceSyncActive(): boolean {
  return unifiedSyncTransport !== null && !_sessionCode;
}

/**
 * Check if workspace is currently initializing.
 */
export function isInitializing(): boolean {
  return _initializing;
}

// ===========================================================================
// Initialization
// ===========================================================================

export interface WorkspaceInitOptions {
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Server URL (optional) */
  serverUrl?: string;
  /** Workspace ID for room naming */
  workspaceId?: string;
  /** Called when initialization completes */
  onReady?: () => void;
  /** Called when file metadata changes */
  onFileChange?: FileChangeCallback;
}

/**
 * Initialize the workspace CRDT.
 */
export async function initWorkspace(options: WorkspaceInitOptions): Promise<void> {
  if (initialized) {
    console.warn('[WorkspaceCrdtBridge] Already initialized');
    return;
  }

  _initializing = true;

  try {
    rustApi = options.rustApi;
    // Keep existing serverUrl if set (from setWorkspaceServer called before init)
    if (options.serverUrl) {
      serverUrl = options.serverUrl;
      _serverUrl = toWebSocketUrl(options.serverUrl);
    }
    _workspaceId = options.workspaceId ?? null;

    console.log('[WorkspaceCrdtBridge] initWorkspace:', {
      workspaceId: _workspaceId,
      serverUrl: serverUrl,
      resolvedServerUrl: _serverUrl,
      hasRustApi: !!rustApi,
    });

    if (options.onFileChange) {
      fileChangeCallbacks.add(options.onFileChange);
    }

    // Connect sync if we have a workspaceId (authenticated mode, not local-only)
    if (_workspaceId) {
      if (serverUrl && rustApi && _backend) {
        if (_freshFromServerLoad) {
          await resetBodyDocsForFreshServerLoad();
        }

        const workspaceDocName = _workspaceId ? `${_workspaceId}:workspace` : 'workspace';

        // Check if backend supports native sync (Tauri)
        if (_backend.hasNativeSync?.() && _backend.startSync) {
          // Use _serverUrl (WebSocket URL) for native sync - Rust client expects wss:// scheme
          console.log('[WorkspaceCrdtBridge] Using native sync during init (Tauri):', _serverUrl, 'docName:', workspaceDocName);

          // Set up event listener for native sync events
          if (_backend.onSyncEvent) {
            _nativeSyncUnsubscribe = _backend.onSyncEvent((event: SyncEvent) => {
              handleNativeSyncEvent(event);
            });
          }

          try {
            _nativeSyncActive = true;
            await _backend.startSync(_serverUrl!, workspaceDocName, getToken() ?? undefined);
            console.log('[WorkspaceCrdtBridge] Native sync started successfully (init)');
          } catch (e) {
            console.error('[WorkspaceCrdtBridge] Native sync failed to start (init):', e);
            _nativeSyncActive = false;
          }
        } else {
          // Use UnifiedSyncTransport (single WebSocket for workspace + body via /sync2)
          console.log('[WorkspaceCrdtBridge] Using UnifiedSyncTransport during init');

          const v2Url = toWebSocketUrl(serverUrl);

          unifiedSyncTransport = createUnifiedSyncTransport({
            serverUrl: v2Url,
            workspaceId: _workspaceId!,
            backend: _backend,
            writeToDisk: true,
            authToken: getToken() ?? undefined,
            onStatusChange: (connected) => {
              notifySyncStatus(connected ? 'syncing' : 'idle');
              if (!connected) {
                collaborationStore.setBodySyncStatus('idle');
              }
            },
            onWorkspaceSynced: async () => {
              notifySyncStatus('synced');
              notifyFileChange(null, null);
              await updateFileIndexFromCrdt();
              markInitialSyncComplete();
              // Body sync is handled automatically by SyncSession in Rust
              _freshFromServerLoad = false;
              collaborationStore.setBodySyncStatus('synced');
            },
            onSyncComplete: (filesSynced) => {
              console.log(`[UnifiedSync] Sync complete (init): ${filesSynced} files synced`);
              collaborationStore.setBodySyncStatus('synced');
            },
            onFilesChanged: async () => {
              notifyFileChange(null, null);
            },
            onBodyChanged: (filePath) => {
              if (rustApi) {
                rustApi.getBodyContent(filePath).then(content => {
                  if (content) {
                    notifyBodyChange(filePath, content);
                  }
                }).catch(e => {
                  console.warn('[WorkspaceCrdtBridge] Failed to get body content:', e);
                });
              }
            },
            onProgress: (completed, total) => {
              notifySyncProgress(completed, total);
            },
          });

          notifySyncStatus('connecting');
          await unifiedSyncTransport.connect();
          await flushPendingLocalSyncUpdates();
        }
      }
    } else {
      console.log('[WorkspaceCrdtBridge] Sync skipped: local-only mode (no workspaceId)');
      // No sync needed - mark as complete immediately
      markInitialSyncComplete();
    }

    initialized = true;
    registerBridgeOnGlobal();
    options.onReady?.();
  } finally {
    _initializing = false;
  }
}


/**
 * Disconnect the workspace sync.
 */
export function disconnectWorkspace(): void {
  if (unifiedSyncTransport) {
    unifiedSyncTransport.destroy();
    unifiedSyncTransport = null;
  }
}

/**
 * Destroy the workspace and cleanup.
 */
export async function destroyWorkspace(): Promise<void> {
  // Disconnect existing sync (native or browser-based)
  await disconnectExistingSync();

  discardQueuedLocalSyncUpdates('destroyWorkspace');

  // Clear pending intervals/timeouts to prevent memory leaks
  for (const interval of pendingIntervals) {
    clearInterval(interval);
  }
  pendingIntervals.clear();

  for (const timeout of pendingTimeouts) {
    clearTimeout(timeout);
  }
  pendingTimeouts.clear();

  // Clear file locks
  fileLocks.clear();

  rustApi = null;
  initialized = false;
  fileChangeCallbacks.clear();
}

/**
 * Check if workspace is initialized.
 */
export function isWorkspaceInitialized(): boolean {
  return initialized;
}

/**
 * Check if workspace is connected to server.
 */
export function isWorkspaceConnected(): boolean {
  // Check native sync first (Tauri)
  if (_nativeSyncActive) {
    return true;
  }
  // Check v2 unified transport
  return unifiedSyncTransport !== null;
}

// ===========================================================================
// File Operations
// ===========================================================================

/**
 * Get file metadata from the CRDT.
 */
export async function getFileMetadata(path: string): Promise<FileMetadata | null> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.getFileMetadata) {
    return _g.__diaryx_bridge.getFileMetadata(path);
  }
  return _getFileMetadataImpl(path);
}

async function _getFileMetadataImpl(path: string): Promise<FileMetadata | null> {
  if (!rustApi) return null;
  return rustApi.getFile(path);
}

/**
 * Get all files (excluding deleted).
 */
export async function getAllFiles(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(false);
  return new Map(files);
}

/**
 * Get all files including deleted.
 */
export async function getAllFilesIncludingDeleted(): Promise<Map<string, FileMetadata>> {
  if (!rustApi) return new Map();
  const files = await rustApi.listFiles(true);
  return new Map(files);
}

/**
 * Mark all CRDT files as deleted (tombstone).
 * Used when "Load from server" is selected to ensure local files don't
 * persist after sync with server.
 *
 * Note: This sets deleted=true on all files but preserves other metadata
 * as required by CRDT semantics.
 */
export async function markAllCrdtFilesAsDeleted(): Promise<number> {
  if (!rustApi) {
    console.warn('[WorkspaceCrdtBridge] Cannot mark files as deleted: not initialized');
    return 0;
  }

  const files = await rustApi.listFiles(false); // Get non-deleted files
  const filePaths = files.map(([path]) => path);

  console.log(`[WorkspaceCrdtBridge] Marking ${filePaths.length} CRDT files as deleted`);

  let deleted = 0;
  for (const path of filePaths) {
    try {
      const existing = await rustApi.getFile(path);
      if (existing && !existing.deleted) {
        const updated: FileMetadata = {
          ...existing,
          deleted: true,
          modified_at: BigInt(Date.now()),
        };
        await rustApi.setFile(path, updated);
        deleted++;
      }
    } catch (e) {
      console.warn(`[WorkspaceCrdtBridge] Failed to mark ${path} as deleted:`, e);
    }
  }

  console.log(`[WorkspaceCrdtBridge] Marked ${deleted} CRDT files as deleted`);
  return deleted;
}

/**
 * TreeNode interface (matching the backend interface)
 */
interface TreeNode {
  name: string;
  description: string | null;
  path: string;
  children: TreeNode[];
}

/**
 * Populate the CRDT with file metadata.
 * Used before creating a share session to ensure all files are in the CRDT.
 *
 * @param files - Files to add to the CRDT
 * @param externalRustApi - Optional RustCrdtApi to use (for pre-init population)
 */
export async function populateCrdtFromFiles(
  files: Array<{ path: string; metadata: Partial<FileMetadata> }>,
  externalRustApi?: RustCrdtApi
): Promise<void> {
  const api = externalRustApi ?? rustApi;
  if (!api) {
    console.error('[WorkspaceCrdtBridge] Cannot populate CRDT: not initialized');
    return;
  }

  console.log('[WorkspaceCrdtBridge] Populating CRDT with', files.length, 'files');

  for (const { path, metadata } of files) {
    const fullMetadata: FileMetadata = {
      filename: path.split('/').pop() ?? '',
      title: metadata.title ?? null,
      part_of: metadata.part_of ?? null,
      contents: metadata.contents ?? null,
      attachments: metadata.attachments ?? [],
      deleted: metadata.deleted ?? false,
      audience: metadata.audience ?? null,
      description: metadata.description ?? null,
      extra: metadata.extra ?? {},
      modified_at: metadata.modified_at ?? BigInt(Date.now()),
    };

    await api.setFile(path, fullMetadata);
  }

  console.log('[WorkspaceCrdtBridge] CRDT population complete with', files.length, 'files');
}

/**
 * Build a tree from CRDT file metadata.
 * This is used for guests who don't have files on disk but have synced metadata.
 *
 * Both hosts and guests read from Rust CRDT - the sync mechanism (RustSyncBridge)
 * updates the Rust CRDT directly, so it contains the synced data for both.
 */
export async function getTreeFromCrdt(): Promise<TreeNode | null> {
  // Both hosts and guests read from Rust CRDT
  // (sync updates Rust CRDT directly via RustSyncBridge)
  if (!rustApi) return null;

  const files = await rustApi.listFiles(false);

  if (files.length === 0) return null;

  const fileMap = new Map(files);
  const knownPaths = new Set(fileMap.keys());
  console.log('[WorkspaceCrdtBridge] Building tree from CRDT, files:', files.map(([p]) => p));

  async function runLinkParser(
    operation: unknown
  ): Promise<{ type: 'parsed' | 'string'; data: unknown } | null> {
    if (!_backend) return null;
    try {
      const response = await _backend.execute({
        type: 'LinkParser' as any,
        params: { operation } as any,
      } as any);
      if ((response as any).type !== 'LinkParserResult') return null;
      return (response as any).data;
    } catch {
      return null;
    }
  }

  async function resolveReferencePath(path: string, reference: string | null): Promise<string | null> {
    if (!reference) return null;

    const parsed = await runLinkParser({
      type: 'parse',
      params: { link: reference },
    });

    // If backend parser is unavailable, use local resolver.
    if (!parsed || parsed.type !== 'parsed') {
      return resolveCrdtReferencePath(path, reference, knownPaths);
    }

    const parsedData = parsed.data as { path_type?: string };

    // Ambiguous links: compute both interpretations and disambiguate against known paths.
    if (parsedData.path_type === 'ambiguous') {
      const relativeResolved = await runLinkParser({
        type: 'to_canonical',
        params: {
          link: reference,
          current_file_path: path,
          link_format_hint: null,
        },
      });
      const rootResolved = await runLinkParser({
        type: 'to_canonical',
        params: {
          link: reference,
          current_file_path: path,
          link_format_hint: 'plain_canonical',
        },
      });

      const relativePath =
        relativeResolved?.type === 'string' ? normalizeWorkspacePath(String(relativeResolved.data)) : null;
      const rootPath =
        rootResolved?.type === 'string' ? normalizeWorkspacePath(String(rootResolved.data)) : null;

      if (relativePath && rootPath && relativePath !== rootPath) {
        const relativeExists = knownPaths.has(relativePath);
        const rootExists = knownPaths.has(rootPath);
        if (rootExists && !relativeExists) return rootPath;
        if (relativeExists && !rootExists) return relativePath;
      }
      return rootPath ?? relativePath ?? resolveCrdtReferencePath(path, reference, knownPaths);
    }

    const canonical = await runLinkParser({
      type: 'to_canonical',
      params: {
        link: reference,
        current_file_path: path,
        link_format_hint: null,
      },
    });
    if (canonical?.type === 'string') {
      return normalizeWorkspacePath(String(canonical.data));
    }

    return resolveCrdtReferencePath(path, reference, knownPaths);
  }

  // Helper to check if a part_of reference is valid.
  async function hasValidPartOf(path: string, partOf: string | null): Promise<boolean> {
    const resolved = await resolveReferencePath(path, partOf);
    return !!resolved && fileMap.has(resolved);
  }

  // Find root files (files with no part_of, or part_of pointing to non-existent file)
  const rootFiles: string[] = [];
  for (const [path, metadata] of fileMap) {
    if (!(await hasValidPartOf(path, metadata.part_of))) {
      rootFiles.push(path);
    }
  }

  console.log('[WorkspaceCrdtBridge] Root files:', rootFiles);

  // If no clear root, use the first file as root
  if (rootFiles.length === 0 && files.length > 0) {
    rootFiles.push(files[0][0]);
  }

  // Build tree recursively
  // For guests, paths are prefixed with guest/{joinCode}/ to point to isolated storage
  async function buildNode(originalPath: string): Promise<TreeNode> {
    const metadata = fileMap.get(originalPath);
    const name = originalPath.split('/').pop()?.replace(/\.md$/, '') || originalPath;

    // For guests, use prefixed path so file opens work correctly
    const storagePath = getGuestStoragePath(originalPath);

    const children: TreeNode[] = [];
    if (metadata?.contents) {
      for (const childPath of metadata.contents) {
        const resolvedChildPath = await resolveReferencePath(originalPath, childPath);
        if (resolvedChildPath && fileMap.has(resolvedChildPath)) {
          children.push(await buildNode(resolvedChildPath));
        } else if (fileMap.has(childPath)) {
          // Legacy fallback: treat existing value as canonical path.
          children.push(await buildNode(childPath));
        }
      }
    }

    return {
      name: metadata?.title || name,
      description: metadata?.description || null,
      path: storagePath,  // Use storage path (prefixed for guests)
      children,
    };
  }

  // If single root, return it; otherwise create a virtual root
  if (rootFiles.length === 1) {
    return await buildNode(rootFiles[0]);
  } else {
    // Multiple roots - create a virtual workspace root
    const virtualRootPath = getGuestStoragePath('workspace');
    return {
      name: 'Shared Workspace',
      description: 'Files shared in this session',
      path: virtualRootPath,
      children: await Promise.all(rootFiles.map((root) => buildNode(root))),
    };
  }
}

/**
 * Set file metadata in the CRDT.
 */
export async function setFileMetadata(path: string, metadata: FileMetadata): Promise<void> {
  console.log('[WorkspaceCrdtBridge] setFileMetadata called:', path);
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  // Skip temporary files - they should never enter the CRDT
  if (isTempFile(path)) {
    console.log('[WorkspaceCrdtBridge] Skipping setFileMetadata for temp file:', path);
    return;
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked setFileMetadata in read-only session:', path);
    return;
  }

  await rustApi.setFile(path, metadata);
  // Sync happens automatically via SendSyncMessage events from Rust

  console.log('[WorkspaceCrdtBridge] setFileMetadata complete:', path);
  notifyFileChange(path, metadata);
}

/**
 * Check if updates should be blocked due to read-only mode.
 * Returns true if the user is a guest in a read-only session.
 */
function isReadOnlyBlocked(): boolean {
  return shareSessionStore.isGuest && shareSessionStore.readOnly;
}

/**
 * Close body sync for a specific file.
 * Body sync is now automatic via SyncSession, so this is a no-op.
 *
 * @deprecated Body sync lifecycle is managed by SyncSession.
 */
export function closeBodySync(_filePath: string): void {
  // Body sync lifecycle is managed by SyncSession in Rust.
}

/**
 * Ensure body sync bridge is connected for a file.
 * Call this when opening a file to receive remote body updates.
 *
 * This eagerly creates the body bridge so that remote body updates
 * are received even before the user starts editing. Without this,
 * files opened from sync would appear empty because the body bridge
 * wasn't created yet.
 *
 * NOTE: When backend has native sync capability (Tauri), this is a no-op because
 * the native SyncClient handles body sync internally.
 */
export async function ensureBodySync(filePath: string): Promise<void> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.ensureBodySync) {
    return _g.__diaryx_bridge.ensureBodySync(filePath);
  }
  return _ensureBodySyncImpl(filePath);
}

async function _ensureBodySyncImpl(_filePath: string): Promise<void> {
  // Body sync is now automatic via SyncSession in Rust.
  // All body files are synced after the workspace handshake completes.
}

/**
 * Get body content from the CRDT.
 * This is useful for guests who don't have files on disk but need to read
 * body content that was synced into the CRDT.
 *
 * @param filePath - The file path (can be storage path - will be converted to canonical)
 * @returns The body content, or null if not available
 */
export async function getBodyContentFromCrdt(filePath: string): Promise<string | null> {
  // Delegate to configured module instance if this one isn't configured
  if (!rustApi && _g.__diaryx_bridge?.getBodyContentFromCrdt) {
    return _g.__diaryx_bridge.getBodyContentFromCrdt(filePath);
  }
  return _getBodyContentFromCrdtImpl(filePath);
}

async function _getBodyContentFromCrdtImpl(filePath: string): Promise<string | null> {
  if (!rustApi) {
    return null;
  }
  const canonicalPath = await getCanonicalPathForSync(filePath);
  try {
    const content = await rustApi.getBodyContent(canonicalPath);
    return content || null;
  } catch (err) {
    console.warn('[WorkspaceCrdtBridge] Failed to get body content from CRDT:', err);
    return null;
  }
}

/**
 * Options for proactive body sync.
 */
export interface ProactiveSyncOptions {
  /** How many body syncs to run in parallel (default 3) */
  concurrency?: number;
  /** Callback for progress updates during subscription phase */
  onProgress?: (completed: number, total: number) => void;
  /** Whether to wait for sync_complete from server (default true) */
  waitForComplete?: boolean;
  /** Timeout for waiting for sync_complete in ms (default 120000 = 2 minutes) */
  syncTimeout?: number;
}

/**
 * Proactively sync body docs for multiple files.
 * Body sync is now automatic via SyncSession in Rust, so this is a no-op.
 *
 * @deprecated Body sync is automatic via SyncSession.
 */
export async function proactivelySyncBodies(
  _filePaths: string[],
  _optionsOrConcurrency?: number | ProactiveSyncOptions
): Promise<void> {
  // Body sync is automatic via SyncSession — all body files are synced
  // after the workspace handshake completes.
}



/**
 * Update specific fields of file metadata.
 * Uses a per-file lock to prevent race conditions on concurrent updates.
 */
export async function updateFileMetadata(
  path: string,
  updates: Partial<FileMetadata>
): Promise<void> {
  console.log('[WorkspaceCrdtBridge] updateFileMetadata called:', path, updates);
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  // Block updates in read-only mode for guests
  if (isReadOnlyBlocked()) {
    console.log('[WorkspaceCrdtBridge] Blocked updateFileMetadata in read-only session:', path);
    return;
  }

  // Acquire lock to prevent concurrent read-modify-write races
  const releaseLock = await acquireFileLock(path);

  try {
    const existing = await rustApi.getFile(path);

    // Build updated metadata (without modified_at initially)
    const newTitle = updates.title ?? existing?.title ?? null;
    const newPartOf = updates.part_of ?? existing?.part_of ?? null;
    const newContents = updates.contents ?? existing?.contents ?? null;
    const newAttachments = updates.attachments ?? existing?.attachments ?? [];
    const newDeleted = updates.deleted ?? existing?.deleted ?? false;
    const newAudience = updates.audience ?? existing?.audience ?? null;
    const newDescription = updates.description ?? existing?.description ?? null;
    const newExtra = updates.extra ?? existing?.extra ?? {};

    // Check if there are actual changes (excluding modified_at)
    const hasChanges = existing === null ||
      newTitle !== existing.title ||
      newPartOf !== existing.part_of ||
      newContents !== existing.contents ||
      JSON.stringify(newAttachments) !== JSON.stringify(existing.attachments) ||
      newDeleted !== existing.deleted ||
      newAudience !== existing.audience ||
      newDescription !== existing.description ||
      JSON.stringify(newExtra) !== JSON.stringify(existing.extra);

    if (!hasChanges) {
      console.log('[WorkspaceCrdtBridge] No changes detected, skipping update:', path);
      return;
    }

    const updated: FileMetadata = {
      filename: existing?.filename ?? path.split('/').pop() ?? '',
      title: newTitle,
      part_of: newPartOf,
      contents: newContents,
      attachments: newAttachments,
      deleted: newDeleted,
      audience: newAudience,
      description: newDescription,
      extra: newExtra,
      modified_at: BigInt(Date.now()),
    };

    console.log('[WorkspaceCrdtBridge] Updating file metadata:', path, updated);
    await rustApi.setFile(path, updated);
    console.log('[WorkspaceCrdtBridge] File metadata updated successfully:', path);
    // Sync happens automatically via SendSyncMessage events from Rust

    notifyFileChange(path, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Delete a file (soft delete via tombstone).
 */
export async function deleteFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: true });
}

/**
 * Restore a deleted file.
 */
export async function restoreFile(path: string): Promise<void> {
  await updateFileMetadata(path, { deleted: false });
}

/**
 * Permanently remove a file from the CRDT.
 * Note: This sets all fields to null/empty, as CRDTs don't support true deletion.
 */
export async function purgeFile(path: string): Promise<void> {
  if (!rustApi) {
    throw new Error('Workspace not initialized');
  }

  const metadata: FileMetadata = {
    filename: '',
    title: null,
    part_of: null,
    contents: null,
    attachments: [],
    deleted: true,
    audience: null,
    description: null,
    extra: {},
    modified_at: BigInt(Date.now()),
  };

  await rustApi.setFile(path, metadata);
  // Sync happens automatically via SendSyncMessage events from Rust

  notifyFileChange(path, null);
}

// ===========================================================================
// Hierarchy Operations
// ===========================================================================

/**
 * Add a child to a parent's contents array.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function addToContents(parentPath: string, childPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(parentPath);
  try {
    const parent = await rustApi.getFile(parentPath);
    if (!parent) return;

    const contents = parent.contents ?? [];
    if (!contents.includes(childPath)) {
      contents.push(childPath);
      const updated: FileMetadata = {
        ...parent,
        contents,
        modified_at: BigInt(Date.now()),
      };
      await rustApi.setFile(parentPath, updated);
      // Sync happens automatically via SendSyncMessage events from Rust

      notifyFileChange(parentPath, updated);
    }
  } finally {
    releaseLock();
  }
}

/**
 * Remove a child from a parent's contents array.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function removeFromContents(parentPath: string, childPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(parentPath);
  try {
    const parent = await rustApi.getFile(parentPath);
    if (!parent) return;

    const contents = parent.contents ?? [];
    const index = contents.indexOf(childPath);
    if (index !== -1) {
      contents.splice(index, 1);
      const updated: FileMetadata = {
        ...parent,
        contents: contents.length > 0 ? contents : null,
        modified_at: BigInt(Date.now()),
      };
      await rustApi.setFile(parentPath, updated);
      // Sync happens automatically via SendSyncMessage events from Rust

      notifyFileChange(parentPath, updated);
    }
  } finally {
    releaseLock();
  }
}

/**
 * Set the part_of (parent) for a file.
 */
export async function setPartOf(childPath: string, parentPath: string | null): Promise<void> {
  await updateFileMetadata(childPath, { part_of: parentPath });
}

/**
 * Move a file to a new parent.
 */
export async function moveFile(
  path: string,
  newParentPath: string,
  newPath: string
): Promise<void> {
  const file = await getFileMetadata(path);
  if (!file) return;

  // Remove from old parent
  if (file.part_of) {
    await removeFromContents(file.part_of, path);
  }

  // Add to new parent
  await addToContents(newParentPath, newPath);

  // Update the file's part_of
  if (path !== newPath) {
    // If path changed, create new entry and delete old
    await setFileMetadata(newPath, { ...file, part_of: newParentPath });
    await purgeFile(path);
  } else {
    await updateFileMetadata(path, { part_of: newParentPath });
  }
}

/**
 * Rename a file (change its path).
 */
export async function renameFile(oldPath: string, newPath: string): Promise<void> {
  const file = await getFileMetadata(oldPath);
  if (!file) return;

  // Create new entry with new path
  await setFileMetadata(newPath, { ...file, modified_at: BigInt(Date.now()) });

  // Update parent's contents
  if (file.part_of) {
    const parent = await getFileMetadata(file.part_of);
    if (parent?.contents) {
      const contents = parent.contents.map((c) => (c === oldPath ? newPath : c));
      await updateFileMetadata(file.part_of, { contents });
    }
  }

  // Delete old entry
  await purgeFile(oldPath);
}

// ===========================================================================
// Attachment Operations
// ===========================================================================

/**
 * Add an attachment to a file.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function addAttachment(filePath: string, attachment: BinaryRef): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(filePath);
  try {
    const file = await rustApi.getFile(filePath);
    if (!file) return;

    const attachments = [...file.attachments, attachment];
    const updated: FileMetadata = {
      ...file,
      attachments,
      modified_at: BigInt(Date.now()),
    };
    await rustApi.setFile(filePath, updated);
    // Sync happens automatically via SendSyncMessage events from Rust

    notifyFileChange(filePath, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Remove an attachment from a file.
 * Uses locking to prevent race conditions with concurrent modifications.
 */
export async function removeAttachment(filePath: string, attachmentPath: string): Promise<void> {
  if (!rustApi) return;

  const releaseLock = await acquireFileLock(filePath);
  try {
    const file = await rustApi.getFile(filePath);
    if (!file) return;

    const attachments = file.attachments.filter((a) => a.path !== attachmentPath);
    const updated: FileMetadata = {
      ...file,
      attachments,
      modified_at: BigInt(Date.now()),
    };
    await rustApi.setFile(filePath, updated);
    // Sync happens automatically via SendSyncMessage events from Rust

    notifyFileChange(filePath, updated);
  } finally {
    releaseLock();
  }
}

/**
 * Get attachments for a file.
 */
export async function getAttachments(filePath: string): Promise<BinaryRef[]> {
  const file = await getFileMetadata(filePath);
  return file?.attachments ?? [];
}

// ===========================================================================
// Sync Operations
// ===========================================================================

/**
 * Save CRDT state to storage.
 */
export async function saveCrdtState(): Promise<void> {
  if (!rustApi) return;
  await rustApi.saveCrdtState('workspace');
}

/**
 * Wait for initial sync to complete.
 *
 * This should be called after initWorkspace() to ensure synced data
 * is available before building the UI tree. Returns immediately if:
 * - Sync is already complete
 * - Sync is not enabled (local-only mode)
 *
 * @param timeoutMs Maximum time to wait for sync (default 10 seconds)
 * @returns true if sync completed, false if timed out or not applicable
 */
export function waitForInitialSync(timeoutMs = 10000): Promise<boolean> {
  return new Promise((resolve) => {
    // Already synced
    if (_initialSyncComplete) {
      resolve(true);
      return;
    }

    // No sync in progress (local-only mode or sync disabled)
    if (!unifiedSyncTransport && !_nativeSyncActive && initialized) {
      console.log('[WorkspaceCrdtBridge] waitForInitialSync: no sync transport, resolving immediately');
      _initialSyncComplete = true;
      resolve(true);
      return;
    }

    // Not initialized yet - wait for it
    if (!initialized && !_initializing) {
      console.log('[WorkspaceCrdtBridge] waitForInitialSync: not initialized, resolving false');
      resolve(false);
      return;
    }

    // Set up timeout
    const timeout = setTimeout(() => {
      console.warn('[WorkspaceCrdtBridge] waitForInitialSync: timed out after', timeoutMs, 'ms');
      // Remove our resolver from the list
      const idx = _initialSyncResolvers.indexOf(resolveSync);
      if (idx >= 0) {
        _initialSyncResolvers.splice(idx, 1);
      }
      resolve(false);
    }, timeoutMs);

    // Add resolver to be called when sync completes
    const resolveSync = () => {
      clearTimeout(timeout);
      resolve(true);
    };
    _initialSyncResolvers.push(resolveSync);
  });
}

/**
 * Mark initial sync as complete and notify any waiters.
 * Called internally when the sync bridge's onSynced callback fires.
 */
function markInitialSyncComplete(): void {
  if (_initialSyncComplete) return;

  console.log('[WorkspaceCrdtBridge] Initial sync complete, notifying', _initialSyncResolvers.length, 'waiters');
  _initialSyncComplete = true;

  // Resolve all waiting promises
  for (const resolve of _initialSyncResolvers) {
    resolve();
  }
  _initialSyncResolvers = [];
}

/**
 * Wait for sync to complete (with timeout).
 */
export function waitForSync(timeoutMs = 5000): Promise<boolean> {
  return new Promise((resolve) => {
    if (isWorkspaceConnected()) {
      resolve(true);
      return;
    }

    const cleanup = () => {
      clearInterval(checkInterval);
      clearTimeout(timeout);
      pendingIntervals.delete(checkInterval);
      pendingTimeouts.delete(timeout);
    };

    const timeout = setTimeout(() => {
      cleanup();
      resolve(false);
    }, timeoutMs);
    pendingTimeouts.add(timeout);

    const checkInterval = setInterval(() => {
      if (isWorkspaceConnected()) {
        cleanup();
        resolve(true);
      }
    }, 100);
    pendingIntervals.add(checkInterval);
  });
}

// ===========================================================================
// Statistics
// ===========================================================================

/**
 * Get workspace statistics.
 */
export async function getWorkspaceStats(): Promise<{
  totalFiles: number;
  activeFiles: number;
  deletedFiles: number;
}> {
  const allFiles = await getAllFilesIncludingDeleted();
  const activeFiles = await getAllFiles();

  return {
    totalFiles: allFiles.size,
    activeFiles: activeFiles.size,
    deletedFiles: allFiles.size - activeFiles.size,
  };
}

// ===========================================================================
// Callbacks
// ===========================================================================

/**
 * Subscribe to file changes.
 */
export function onFileChange(callback: FileChangeCallback): () => void {
  fileChangeCallbacks.add(callback);
  return () => fileChangeCallbacks.delete(callback);
}

/**
 * Subscribe to file rename events.
 */
export function onFileRenamed(callback: FileRenamedCallback): () => void {
  fileRenamedCallbacks.add(callback);
  return () => fileRenamedCallbacks.delete(callback);
}

/**
 * Subscribe to session sync events.
 * Called when session data is received and synced to Rust.
 * Use this to trigger UI refreshes after receiving data from a share session.
 */
export function onSessionSync(callback: SessionSyncCallback): () => void {
  sessionSyncCallbacks.add(callback);
  return () => sessionSyncCallbacks.delete(callback);
}

/**
 * Subscribe to file body changes.
 * Called when a file's body content changes remotely (from another session participant).
 * Use this to reload the editor when the current file's content changes.
 *
 * @param callback - Receives the canonical path and new body content
 * @returns Unsubscribe function
 */
export function onBodyChange(callback: BodyChangeCallback): () => void {
  bodyChangeCallbacks.add(callback);
  return () => bodyChangeCallbacks.delete(callback);
}

/**
 * Subscribe to sync progress updates.
 * Called when files are being synced to report progress.
 *
 * @param callback - Receives (completed, total) file counts
 * @returns Unsubscribe function
 */
export function onSyncProgress(callback: SyncProgressCallback): () => void {
  syncProgressCallbacks.add(callback);
  return () => syncProgressCallbacks.delete(callback);
}

/**
 * Subscribe to sync status changes.
 * Called when sync status changes (idle, connecting, syncing, synced, error).
 *
 * @param callback - Receives the new status and optional error message
 * @returns Unsubscribe function
 */
export function onSyncStatus(callback: SyncStatusCallback): () => void {
  syncStatusCallbacks.add(callback);
  return () => syncStatusCallbacks.delete(callback);
}

/**
 * Notify all sync status callbacks.
 */
function notifySyncStatus(status: 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', error?: unknown): void {
  // Convert error to string if it's not already (defensive handling for Rust objects)
  let errorStr: string | undefined;
  if (error !== undefined && error !== null) {
    if (typeof error === 'string') {
      errorStr = error;
    } else if (error instanceof Error) {
      errorStr = error.message;
    } else if (typeof error === 'object') {
      const errObj = error as Record<string, unknown>;
      if (typeof errObj.message === 'string') {
        errorStr = errObj.message;
      } else if (typeof errObj.error === 'string') {
        errorStr = errObj.error;
      } else {
        try {
          errorStr = JSON.stringify(error);
        } catch {
          errorStr = 'Unknown error';
        }
      }
    } else {
      errorStr = String(error);
    }
  }

  // Only log sync status changes when there's an error or significant status change
  if (errorStr || status === 'synced' || status === 'error') {
    console.log('[WorkspaceCrdtBridge] Sync status:', status, errorStr ? `(${errorStr})` : '');
  }

  // Update collaborationStore for SyncStatusIndicator
  collaborationStore.setSyncStatus(status);
  if (errorStr) {
    collaborationStore.setSyncError(errorStr);
  } else if (status === 'synced' || status === 'idle') {
    collaborationStore.setSyncProgress(null); // Clear progress when done
  }

  for (const callback of syncStatusCallbacks) {
    try {
      callback(status, errorStr);
    } catch (err) {
      console.error('[WorkspaceCrdtBridge] Sync status callback error:', err);
    }
  }
}

/**
 * Notify all sync progress callbacks.
 */
function notifySyncProgress(completed: number, total: number): void {
  // Update collaborationStore for SyncStatusIndicator
  collaborationStore.setSyncProgress({ completed, total });

  for (const callback of syncProgressCallbacks) {
    try {
      callback(completed, total);
    } catch (err) {
      console.error('[WorkspaceCrdtBridge] Sync progress callback error:', err);
    }
  }
}

/**
 * Notify all session sync callbacks (debounced & re-entrancy guarded).
 *
 * Uses a trailing-edge debounce: multiple calls within DEBOUNCE_MS are
 * coalesced into one notification fired DEBOUNCE_MS after the last call.
 * While callbacks are executing, new calls are queued and a single
 * follow-up notification fires after the current batch completes.
 */
let _sessionSyncTimer: ReturnType<typeof setTimeout> | null = null;
let _sessionSyncRunning = false;
let _sessionSyncPending = false;
const SESSION_SYNC_DEBOUNCE_MS = 200;

function notifySessionSync(): void {
  if (_sessionSyncRunning) {
    // A callback is already executing — schedule one follow-up after it finishes
    _sessionSyncPending = true;
    return;
  }
  // Reset and (re)start the debounce timer
  if (_sessionSyncTimer !== null) clearTimeout(_sessionSyncTimer);
  _sessionSyncTimer = setTimeout(() => {
    _sessionSyncTimer = null;
    _fireSessionSyncCallbacks();
  }, SESSION_SYNC_DEBOUNCE_MS);
}

async function _fireSessionSyncCallbacks(): Promise<void> {
  _sessionSyncRunning = true;
  _sessionSyncPending = false;
  console.log('[WorkspaceCrdtBridge] Notifying session sync callbacks, count:', sessionSyncCallbacks.size);
  for (const callback of sessionSyncCallbacks) {
    try {
      await callback();
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] Session sync callback error:', error);
    }
  }
  _sessionSyncRunning = false;
  // If new calls arrived while we were running, fire one more time (debounced)
  if (_sessionSyncPending) {
    _sessionSyncPending = false;
    if (_sessionSyncTimer !== null) clearTimeout(_sessionSyncTimer);
    _sessionSyncTimer = setTimeout(() => {
      _sessionSyncTimer = null;
      _fireSessionSyncCallbacks();
    }, SESSION_SYNC_DEBOUNCE_MS);
  }
}

// Private helpers

function notifyFileChange(path: string | null, metadata: FileMetadata | null): void {
  if (path && isTempFile(path)) {
    return;
  }
  if (path && metadata && _workspaceId && metadata.attachments.length > 0) {
    enqueueMissingDownloadsFromMetadata(path, _workspaceId, metadata.attachments);
  }
  for (const callback of fileChangeCallbacks) {
    try {
      callback(path, metadata);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] File change callback error:', error);
    }
  }
}

function notifyFileRenamed(oldPath: string, newPath: string): void {
  if (isTempFile(oldPath) || isTempFile(newPath)) {
    return;
  }
  for (const callback of fileRenamedCallbacks) {
    try {
      callback(oldPath, newPath);
    } catch (error) {
      console.error('[WorkspaceCrdtBridge] File renamed callback error:', error);
    }
  }
}

/**
 * Update the file index from Rust CRDT state.
 * Called after remote sync updates to keep the SQLite index in sync.
 */
/**
 * Update the SQLite file index from the CRDT state.
 * Includes retry logic for when storage isn't ready yet.
 *
 * @param retryCount Number of retries remaining (default 3)
 * @param retryDelayMs Delay between retries in ms (default 500, doubles each retry)
 */
async function updateFileIndexFromCrdt(retryCount = 3, retryDelayMs = 500): Promise<void> {
  if (!rustApi) return;

  // Check if SQLite storage is initialized
  if (!isStorageReady()) {
    if (retryCount > 0) {
      console.log(`[WorkspaceCrdtBridge] Storage not ready, retrying file index update in ${retryDelayMs}ms (${retryCount} retries left)`);
      setTimeout(() => {
        updateFileIndexFromCrdt(retryCount - 1, retryDelayMs * 2);
      }, retryDelayMs);
    } else {
      console.warn('[WorkspaceCrdtBridge] Storage not ready after retries, skipping file index update');
    }
    return;
  }

  try {
    const files = await rustApi.listFiles(true); // Include deleted to update tombstones
    console.log(`[WorkspaceCrdtBridge] Updating file index with ${files.length} files`);
    for (const [path, metadata] of files) {
      if (metadata) {
        crdt_update_file_index(
          path,
          metadata.title ?? null,
          metadata.part_of ?? null,
          metadata.deleted ?? false,
          Number(metadata.modified_at ?? Date.now())
        );
      }
    }
  } catch (err) {
    console.error('[WorkspaceCrdtBridge] Failed to update file index:', err);
  }
}

// ===========================================================================
// Filesystem Event Subscription
// ===========================================================================

// Active filesystem event subscription ID
let fsEventSubscriptionId: number | null = null;

/**
 * Initialize filesystem event subscription from the Rust backend.
 *
 * This subscribes to events emitted by the decorated filesystem layer
 * (EventEmittingFs/CrdtFs) and uses them to update the UI and CRDT state.
 *
 * Call this after the backend is initialized to enable automatic
 * UI updates when filesystem operations occur.
 *
 * @param backend The backend instance to subscribe to
 * @returns Cleanup function to unsubscribe
 */
export function initEventSubscription(backend: Backend): () => void {
  // Skip if backend doesn't support filesystem events
  if (!backend.onFileSystemEvent) {
    console.log('[WorkspaceCrdtBridge] Backend does not support filesystem events');
    return () => {};
  }

  // Unsubscribe from any existing subscription
  if (fsEventSubscriptionId !== null && backend.offFileSystemEvent) {
    backend.offFileSystemEvent(fsEventSubscriptionId);
  }

  // Subscribe to filesystem events
  fsEventSubscriptionId = backend.onFileSystemEvent((event: FileSystemEvent) => {
    handleFileSystemEvent(event);
  });

  console.log('[WorkspaceCrdtBridge] Subscribed to filesystem events, id:', fsEventSubscriptionId);

  // Return cleanup function
  return () => {
    if (fsEventSubscriptionId !== null && backend.offFileSystemEvent) {
      backend.offFileSystemEvent(fsEventSubscriptionId);
      fsEventSubscriptionId = null;
      console.log('[WorkspaceCrdtBridge] Unsubscribed from filesystem events');
    }
  };
}

/**
 * Handle a filesystem event from the Rust backend.
 *
 * This function processes events and triggers appropriate UI updates
 * and CRDT synchronization.
 */
function handleFileSystemEvent(event: FileSystemEvent): void {
  if (eventTouchesTempFile(event)) {
    return;
  }

  switch (event.type) {
    case 'FileCreated':
      // New file created - notify UI
      notifyFileChange(event.path, event.frontmatter ? (event.frontmatter as FileMetadata) : null);
      // Trigger tree refresh (guests get tree rebuilds from sync transport
      // callbacks onWorkspaceSynced/onFilesChanged, not from filesystem events)
      if (!shareSessionStore.isGuest) {
        notifyFileChange(null, null);
      }
      break;

    case 'FileDeleted':
      // Close body sync bridge for deleted file (cleanup)
      closeBodySync(event.path);
      // File deleted - notify UI with null metadata
      notifyFileChange(event.path, null);
      if (!shareSessionStore.isGuest) {
        notifyFileChange(null, null);
      }
      break;

    case 'FileRenamed':
      // Close body sync bridge for old path (cleanup)
      closeBodySync(event.old_path);
      // File renamed - notify rename listeners so open-entry path can be remapped.
      notifyFileRenamed(event.old_path, event.new_path);

      // Notify old path deletion and new path metadata updates.
      notifyFileChange(event.old_path, null);
      if (rustApi) {
        rustApi
          .getFile(event.new_path)
          .then((metadata) => {
            notifyFileChange(event.new_path, metadata ?? null);
          })
          .catch((error) => {
            console.warn('[WorkspaceCrdtBridge] Failed to read metadata for renamed file:', error);
            notifyFileChange(event.new_path, null);
          });
      } else {
        notifyFileChange(event.new_path, null);
      }
      if (!shareSessionStore.isGuest) {
        notifyFileChange(null, null);
      }
      break;

    case 'FileMoved':
      // Close body sync bridge for old path (cleanup)
      if (event.old_parent !== undefined) {
        const filename = event.path.split('/').pop();
        if (filename) {
          closeBodySync(event.old_parent + '/' + filename);
        }
      }
      // File moved - notify the new path
      notifyFileChange(event.path, null);
      if (!shareSessionStore.isGuest) {
        notifyFileChange(null, null);
      }
      break;

    case 'MetadataChanged':
      // Metadata changed - notify with new frontmatter
      notifyFileChange(event.path, event.frontmatter as FileMetadata);
      break;

    case 'ContentsChanged':
      // Body content changed - notify body change callbacks
      notifyBodyChange(event.path, event.body);
      break;

    // Sync events - use helpers for dispatch
    case 'SyncStarted':
      console.log('[WorkspaceCrdtBridge] Sync started for:', event.doc_name);
      break;

    case 'SyncCompleted':
      console.log('[WorkspaceCrdtBridge] Sync completed for:', event.doc_name, 'files:', event.files_synced);
      markInitialSyncComplete();
      // Update file index after sync
      updateFileIndexFromCrdt();
      break;

    case 'SyncStatusChanged':
      console.log('[WorkspaceCrdtBridge] Sync status changed:', event.status, event.error);
      notifySyncStatus(event.status as 'idle' | 'connecting' | 'syncing' | 'synced' | 'error', event.error);
      break;

    case 'SyncProgress':
      console.log('[WorkspaceCrdtBridge] Sync progress:', event.completed, '/', event.total);
      notifySyncProgress(event.completed, event.total);
      break;

    case 'SendSyncMessage': {
      // Rust is requesting that we send a sync message over WebSocket.
      // This happens after CRDT updates (SaveEntry, CreateEntry, DeleteEntry, RenameEntry).
      //
      // For native sync (Tauri): Skip - native SyncClient handles this internally.
      if (_backend?.hasNativeSync?.()) {
        break;
      }

      const { doc_name, message, is_body } = event as any;
      const bytes = new Uint8Array(message);

      if (!_workspaceId) {
        console.warn('[WorkspaceCrdtBridge] Dropping sync message: missing workspace ID');
        break;
      }

      // Route through WasmSyncClient (Rust handles framing and queuing)
      const docId = is_body
        ? `body:${_workspaceId}/${doc_name}`
        : `workspace:${_workspaceId}`;

      if (unifiedSyncTransport) {
        unifiedSyncTransport.queueLocalUpdate(docId, bytes).catch(err => {
          console.warn('[WorkspaceCrdtBridge] Failed to queue sync message:', err);
        });
      } else {
        // Queue until a transport is connected (reconnect / setup transition).
        pendingLocalSyncUpdates.push({ docId, bytes });
        // Keep bounded to avoid unbounded memory growth if sync is down.
        if (pendingLocalSyncUpdates.length > 2000) {
          pendingLocalSyncUpdates = pendingLocalSyncUpdates.slice(-2000);
        }
      }
      break;
    }
  }
}

/**
 * Check if filesystem event subscription is active.
 */
export function isEventSubscriptionActive(): boolean {
  return fsEventSubscriptionId !== null;
}

// ===========================================================================
// Debug
// ===========================================================================

/**
 * Debug function to check sync state.
 * Call this from browser console: window.debugSync()
 */
export function debugSync(): void {
  console.log('=== Sync Debug ===');
  console.log('serverUrl:', serverUrl);
  console.log('nativeSyncActive:', _nativeSyncActive);
  console.log('unifiedSyncTransport:', unifiedSyncTransport ? 'exists' : 'null');
  console.log('initialized:', initialized);
  console.log('rustApi:', rustApi ? 'exists' : 'null');
  console.log('hasNativeSync:', _backend?.hasNativeSync?.() ?? false);

  if (rustApi) {
    console.log('Fetching Rust CRDT state...');
    rustApi.getFullState('workspace').then(fullState => {
      console.log('Rust CRDT full state:', fullState.length, 'bytes');
      return rustApi!.listFiles(false);
    }).then(files => {
      console.log('Rust CRDT files count:', files.length);
      console.log('Rust CRDT files:', files.map(([path]) => path));
    }).catch(e => {
      console.error('Error getting Rust state:', e);
    });
  }
  console.log('=== End Debug ===');
}

// Expose debug function globally for browser console
if (typeof window !== 'undefined') {
  (window as any).debugSync = debugSync;
}

// Re-export types
export type { FileMetadata, BinaryRef };
