/**
 * Web Worker entry point for DiaryxBackend.
 *
 * This worker uses the new native storage backends (OPFS/IndexedDB) directly,
 * eliminating the need for InMemoryFileSystem and JS↔WASM sync.
 *
 * All operations use the unified `execute()` command API, except:
 * - `getConfig` / `saveConfig`: WASM-specific config stored in root frontmatter
 * - `readBinary` / `writeBinary`: Efficient Uint8Array handling (no base64 overhead)
 */

import * as Comlink from 'comlink';
import type { BackendEventType, BackendEventListener } from './interface';
import type { StorageType } from './storageType';

// We'll dynamically import the WASM module
let backend: any | null = null;

// Discovered workspace root path (set after init)
let rootPath: string | null = null;

// Event port for forwarding filesystem events to main thread
let eventPort: MessagePort | null = null;

// Subscription ID for filesystem events (to clean up on shutdown)
let fsEventSubscriptionId: number | null = null;

// WasmSyncClient instance (created by createSyncClient, lives in worker)
let syncClient: any | null = null;

// Stored init params for lazy CRDT storage setup
let _storedStorageType: StorageType | null = null;
let _storedWorkspaceName: string | null = null;
let _storedDirectoryHandle: FileSystemDirectoryHandle | null = null;
let _crdtStorageReady = false;

// Clear cached root path (call after rename operations)
function clearRootPathCache() {
  rootPath = null;
}

/**
 * Get the backend instance (throws if not initialized).
 */
function getBackend(): any {
  if (!backend) {
    throw new Error('DiaryxBackend not initialized. Call init() first.');
  }
  return backend;
}

/**
 * Execute a command and parse the response.
 * Helper to avoid repetitive JSON.stringify/parse in each method.
 */
async function executeCommand<T = any>(type: string, params?: Record<string, any>): Promise<T> {
  const command = params ? { type, params } : { type };
  const json = await getBackend().execute(JSON.stringify(command));
  return JSON.parse(json);
}

/**
 * Execute a command and extract the data from the response.
 * Throws if the response type doesn't match the expected type.
 */
async function executeAndExtract<T>(
  type: string,
  params: Record<string, any> | undefined,
  expectedResponseType: string
): Promise<T> {
  const response = await executeCommand(type, params);
  if (response.type === expectedResponseType) {
    return response.data as T;
  }
  if (response.type === 'Ok') {
    return undefined as T; // For void responses
  }
  throw new Error(`Expected ${expectedResponseType}, got ${response.type}: ${JSON.stringify(response)}`);
}

/**
 * Migrate legacy OPFS workspace directory to a new name.
 * Copies files from "diaryx/" to the target name, then deletes the old directory.
 * Also migrates legacy ".diaryx/" at OPFS root into the workspace directory.
 */
async function migrateWorkspaceDirectory(targetName: string): Promise<void> {
  if (targetName === 'diaryx') return;

  try {
    const root = await navigator.storage.getDirectory();

    // Check if target already exists — no migration needed
    try {
      await root.getDirectoryHandle(targetName, { create: false });
      // Target exists. Still check for legacy .diaryx/ at root to migrate into workspace.
      await migrateLegacyCrdtDir(root, targetName);
      return;
    } catch {
      // Target doesn't exist, check for old directory
    }

    // Check if old "diaryx" directory exists
    let oldDir: FileSystemDirectoryHandle;
    try {
      oldDir = await root.getDirectoryHandle('diaryx', { create: false });
    } catch {
      // Neither exists — fresh install, also migrate legacy .diaryx/ if present
      await migrateLegacyCrdtDir(root, targetName);
      return;
    }

    // Old directory exists — copy recursively to new name
    console.log(`[WasmWorker] Migrating workspace directory: diaryx -> ${targetName}`);
    const newDir = await root.getDirectoryHandle(targetName, { create: true });
    await copyDirectoryRecursive(oldDir, newDir);
    await root.removeEntry('diaryx', { recursive: true });
    console.log(`[WasmWorker] Migration complete: diaryx -> ${targetName}`);

    // Also migrate legacy .diaryx/ at root
    await migrateLegacyCrdtDir(root, targetName);
  } catch (e) {
    console.error('[WasmWorker] Migration failed:', e);
  }
}

/**
 * Migrate legacy ".diaryx/" directory from OPFS root into the workspace directory.
 */
async function migrateLegacyCrdtDir(root: FileSystemDirectoryHandle, workspaceName: string): Promise<void> {
  try {
    const legacyDir = await root.getDirectoryHandle('.diaryx', { create: false });
    // Legacy .diaryx/ exists at root — copy crdt.db into workspace/.diaryx/
    console.log('[WasmWorker] Migrating legacy .diaryx/ into workspace');
    const wsDir = await root.getDirectoryHandle(workspaceName, { create: true });
    const targetDir = await wsDir.getDirectoryHandle('.diaryx', { create: true });
    await copyDirectoryRecursive(legacyDir, targetDir);
    await root.removeEntry('.diaryx', { recursive: true });
    console.log('[WasmWorker] Legacy .diaryx/ migrated into workspace');
  } catch {
    // No legacy .diaryx/ at root — nothing to do
  }
}

/**
 * Migrate a UUID-named OPFS directory to a name-based one.
 * This handles users who previously had UUID-named directories from workspace switching.
 */
async function migrateUuidToName(workspaceId: string, workspaceName: string): Promise<void> {
  try {
    const root = await navigator.storage.getDirectory();

    // Check if name-based directory already exists
    try {
      await root.getDirectoryHandle(workspaceName, { create: false });
      // Name-based dir already exists — skip migration, just clean up UUID dir if present
      try {
        await root.removeEntry(workspaceId, { recursive: true });
        console.log(`[WasmWorker] Cleaned up orphaned UUID directory: ${workspaceId}`);
      } catch {
        // UUID dir doesn't exist — nothing to clean up
      }
      return;
    } catch {
      // Name-based dir doesn't exist — check for UUID dir to migrate
    }

    // Check if UUID-named directory exists
    let uuidDir: FileSystemDirectoryHandle;
    try {
      uuidDir = await root.getDirectoryHandle(workspaceId, { create: false });
    } catch {
      // Neither exists — nothing to migrate
      return;
    }

    // UUID dir exists, name dir doesn't — copy UUID → name and delete UUID
    console.log(`[WasmWorker] Migrating UUID directory to name: ${workspaceId} -> ${workspaceName}`);
    const nameDir = await root.getDirectoryHandle(workspaceName, { create: true });
    await copyDirectoryRecursive(uuidDir, nameDir);
    await root.removeEntry(workspaceId, { recursive: true });
    console.log(`[WasmWorker] UUID-to-name migration complete: ${workspaceId} -> ${workspaceName}`);
  } catch (e) {
    console.error('[WasmWorker] UUID-to-name migration failed:', e);
  }
}

/**
 * Recursively copy all files and subdirectories from source to dest.
 */
async function copyDirectoryRecursive(
  source: FileSystemDirectoryHandle,
  dest: FileSystemDirectoryHandle,
): Promise<void> {
  for await (const [name, handle] of (source as any).entries()) {
    if (handle.kind === 'file') {
      const file = await (handle as FileSystemFileHandle).getFile();
      const data = await file.arrayBuffer();
      const newFile = await dest.getFileHandle(name, { create: true });
      const writable = await newFile.createWritable();
      await writable.write(data);
      await writable.close();
    } else if (handle.kind === 'directory') {
      const newSubDir = await dest.getDirectoryHandle(name, { create: true });
      await copyDirectoryRecursive(handle as FileSystemDirectoryHandle, newSubDir);
    }
  }
}

/**
 * Set up the CRDT storage bridge (sql.js SQLite).
 * Uses stored init params from init(). Safe to call multiple times (no-op after first).
 */
async function doSetupCrdtStorage(): Promise<void> {
  if (_crdtStorageReady) return;
  if (!_storedStorageType || _storedStorageType === 'memory') return;

  try {
    const { setupCrdtStorageBridge, DirectoryHandlePersistence } = await import('../storage/index.js');

    // Create persistence adapter based on storage type
    let persistence: InstanceType<typeof DirectoryHandlePersistence> | null = null;
    if (_storedStorageType === 'opfs') {
      // Get the workspace directory handle from OPFS for .diaryx/crdt.db persistence
      const opfsRoot = await navigator.storage.getDirectory();
      const wsHandle = await opfsRoot.getDirectoryHandle(_storedWorkspaceName || 'My Journal', { create: true });
      persistence = new DirectoryHandlePersistence(wsHandle);
    } else if (_storedStorageType === 'filesystem-access' && _storedDirectoryHandle) {
      // Use the user's selected directory for .diaryx/crdt.db persistence
      persistence = new DirectoryHandlePersistence(_storedDirectoryHandle);
    }

    await setupCrdtStorageBridge(persistence);
    _crdtStorageReady = true;
    console.log('[WasmWorker] CRDT storage bridge initialized');
  } catch (e) {
    console.warn('[WasmWorker] Failed to initialize CRDT storage bridge, using memory storage:', e);
  }
}

/**
 * Initialize the backend and set up event forwarding.
 */
async function init(port: MessagePort, storageType: StorageType, workspaceName?: string, directoryHandle?: FileSystemDirectoryHandle, syncEnabled?: boolean, workspaceId?: string): Promise<void> {
  // Store event port for forwarding filesystem events
  eventPort = port;

  // Use workspace name for OPFS directory naming (human-readable).
  // The workspaceId is only used for CRDT document namespacing, not storage paths.
  const resolvedWorkspaceName = workspaceName || 'My Journal';

  // Store init params for lazy CRDT storage setup
  _storedStorageType = storageType;
  _storedWorkspaceName = resolvedWorkspaceName;
  _storedDirectoryHandle = directoryHandle ?? null;

  // For OPFS, run legacy "diaryx" -> name migration
  if (storageType === 'opfs') {
    await migrateWorkspaceDirectory(resolvedWorkspaceName);

    // Also migrate UUID-named directories to name-based (for users who had UUID dirs)
    if (workspaceId && workspaceId !== resolvedWorkspaceName) {
      await migrateUuidToName(workspaceId, resolvedWorkspaceName);
    }
  }

  // Initialize CRDT storage bridge BEFORE importing WASM, but only when sync
  // is configured. Without sync, Rust falls back to MemoryStorage which is fine
  // for local-only usage. This avoids downloading sql.js WASM unnecessarily
  // (important for IndexedDB targets where there's no OPFS persistence anyway).
  if (syncEnabled && storageType !== 'memory') {
    await doSetupCrdtStorage();
  }

  // Import WASM module
  // When CDN is configured, load BOTH the JS glue and .wasm binary from CDN
  // to guarantee they come from the same wasm-pack build. wasm-bindgen generates
  // hashed function names that must match between JS and WASM; loading them from
  // different sources (npm JS + CDN WASM) causes "is not a function" errors.
  const wasmCdnUrl = (import.meta as any).env?.VITE_WASM_CDN_URL as string | undefined;
  let wasm: any;
  if (wasmCdnUrl) {
    wasm = await import(/* @vite-ignore */ `${wasmCdnUrl}/diaryx_wasm.js`);
    await wasm.default({ module_or_path: `${wasmCdnUrl}/diaryx_wasm_bg.wasm` });
  } else {
    wasm = await import('@diaryx/wasm');
    await wasm.default();
  }

  // Create backend with specified storage type
  if (storageType === 'opfs') {
    backend = await wasm.DiaryxBackend.createOpfs(resolvedWorkspaceName);
  } else if (storageType === 'filesystem-access') {
    if (!directoryHandle) {
      throw new Error('Directory handle required for filesystem-access storage type');
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createFromDirectoryHandle(directoryHandle);
  } else if (storageType === 'memory') {
    // In-memory storage for guest mode - files live only in memory
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createInMemory();
  } else {
    backend = await wasm.DiaryxBackend.createIndexedDb(workspaceId ?? undefined);
  }

  // Subscribe to filesystem events and forward them to the main thread
  if (backend.onFileSystemEvent && eventPort) {
    fsEventSubscriptionId = backend.onFileSystemEvent((eventJson: string) => {
      // Forward the event JSON to the main thread via MessagePort
      try {
        eventPort!.postMessage({ type: 'FileSystemEvent', data: eventJson });
      } catch (e) {
        console.error('[WasmWorker] Failed to forward filesystem event:', e);
      }
    });
    console.log('[WasmWorker] Subscribed to filesystem events, id:', fsEventSubscriptionId);
  } else {
    console.log('[WasmWorker] Filesystem events not available on this backend');
  }

  console.log('[WasmWorker] DiaryxBackend initialized with storage:', storageType);
}

/**
 * Initialize the backend with a File System Access API directory handle.
 * This is called when the user selects "Local Folder" storage.
 */
async function initWithDirectoryHandle(_port: MessagePort, directoryHandle: FileSystemDirectoryHandle, syncEnabled?: boolean): Promise<void> {
  return init(_port, 'filesystem-access', undefined, directoryHandle, syncEnabled);
}

/**
 * Worker API exposed via Comlink.
 *
 * All methods use the unified command API through `execute()` except where noted.
 */
const workerApi = {
  init,
  initWithDirectoryHandle,

  /**
   * Lazily initialize the CRDT storage bridge (sql.js SQLite).
   * Call this before sync operations if the bridge wasn't set up at init time.
   * No-op if already initialized.
   */
  async setupCrdtStorage(): Promise<void> {
    await doSetupCrdtStorage();
  },

  isReady(): boolean {
    return backend !== null;
  },

  // Event stubs - events will go through MessagePort
  on(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn('[WasmWorker] Events are forwarded via MessagePort, not on()');
  },
  off(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn('[WasmWorker] Events are forwarded via MessagePort, not off()');
  },

  // =========================================================================
  // Config (kept as native calls - WASM-specific frontmatter storage)
  // =========================================================================

  async getConfig(): Promise<any> {
    return getBackend().getConfig();
  },

  async saveConfig(config: any): Promise<void> {
    return getBackend().saveConfig(config);
  },

  // =========================================================================
  // CrdtFs Control
  // =========================================================================

  setCrdtEnabled(enabled: boolean): void {
    getBackend().setCrdtEnabled(enabled);
  },

  isCrdtEnabled(): boolean {
    return getBackend().isCrdtEnabled();
  },

  // =========================================================================
  // Unified Command API
  // =========================================================================

  /**
   * Execute a command using the unified command pattern.
   * Takes JSON string command, returns JSON string response.
   */
  async execute(commandJson: string): Promise<string> {
    return getBackend().execute(commandJson);
  },

  // =========================================================================
  // Root Index Discovery (uses commands)
  // =========================================================================

  async findRootIndex(dirPath?: string): Promise<string | null> {
    const directory = dirPath ?? '.';
    const response = await executeCommand('FindRootIndex', { directory });
    if (response.type === 'String') {
      return response.data;
    }
    return null;
  },

  async getDefaultWorkspacePath(): Promise<string> {
    // Return cached path if available
    if (rootPath) return rootPath;

    // Try to discover root index in current directory first
    let root = await this.findRootIndex('.');

    // Fallback: try "workspace" directory (OPFS default)
    if (!root) {
      root = await this.findRootIndex('workspace');
    }

    // Fallback: scan all top-level directories for a root index
    if (!root) {
      try {
        // Use GetFilesystemTree to list directories
        const response = await executeCommand('GetFilesystemTree', {
          path: '.',
          show_hidden: false,
          depth: 1
        });
        if (response.type === 'Tree' && response.data?.children) {
          for (const child of response.data.children) {
            if (child.children && child.children.length >= 0) {
              // It's a directory
              const found = await this.findRootIndex(child.path);
              if (found) {
                root = found;
                break;
              }
            }
          }
        }
      } catch (e) {
        console.warn('[WasmWorker] Failed to scan directories:', e);
      }
    }

    if (root) {
      // Get parent directory of root index
      const lastSlash = root.lastIndexOf('/');
      const discoveredPath = lastSlash > 0 ? root.substring(0, lastSlash) : '.';
      rootPath = discoveredPath;
      return discoveredPath;
    }

    // Fallback to current directory
    return '.';
  },

  // Clear cached root path (for after rename operations)
  clearRootPathCache(): void {
    clearRootPathCache();
  },

  // =========================================================================
  // Workspace (uses commands)
  // =========================================================================

  async getWorkspaceTree(workspacePath?: string, depth?: number): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('GetWorkspaceTree', { path, depth: depth ?? null }, 'Tree');
  },

  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? '.';
    const workspaceName = name ?? 'My Workspace';
    await executeCommand('CreateWorkspace', { path: workspacePath, name: workspaceName });
    rootPath = workspacePath; // Cache the new workspace path
    return workspacePath;
  },

  async getFilesystemTree(workspacePath?: string, showHidden?: boolean): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('GetFilesystemTree', {
      path,
      show_hidden: showHidden ?? false,
      depth: null
    }, 'Tree');
  },

  // =========================================================================
  // Entries (uses commands)
  // =========================================================================

  async getEntry(path: string): Promise<any> {
    return executeAndExtract('GetEntry', { path }, 'Entry');
  },

  async saveEntry(path: string, content: string): Promise<void> {
    await executeCommand('SaveEntry', { path, content });
  },

  async createEntry(path: string, options?: { title?: string }): Promise<string> {
    return executeAndExtract('CreateEntry', {
      path,
      options: {
        title: options?.title ?? null,
        part_of: null,
        template: null
      }
    }, 'String');
  },

  async deleteEntry(path: string): Promise<void> {
    await executeCommand('DeleteEntry', { path });
  },

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    await executeCommand('MoveEntry', { from: fromPath, to: toPath });
    return toPath;
  },

  async renameEntry(path: string, newFilename: string): Promise<string> {
    const result = await executeAndExtract<string>('RenameEntry', { path, new_filename: newFilename }, 'String');
    // Clear cached root path in case we renamed the root index
    clearRootPathCache();
    return result;
  },

  async duplicateEntry(path: string): Promise<string> {
    return executeAndExtract('DuplicateEntry', { path }, 'String');
  },

  // =========================================================================
  // Frontmatter (uses commands)
  // =========================================================================

  async getFrontmatter(path: string): Promise<any> {
    return executeAndExtract('GetFrontmatter', { path }, 'Frontmatter');
  },

  async setFrontmatterProperty(path: string, key: string, value: any): Promise<void> {
    await executeCommand('SetFrontmatterProperty', { path, key, value });
  },

  // =========================================================================
  // Search (uses commands)
  // =========================================================================

  async searchWorkspace(pattern: string, options?: any): Promise<any> {
    const workspacePath = options?.workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('SearchWorkspace', {
      pattern,
      options: {
        workspace_path: workspacePath,
        search_frontmatter: options?.searchFrontmatter ?? false,
        property: options?.property ?? null,
        case_sensitive: options?.caseSensitive ?? false
      }
    }, 'SearchResults');
  },

  // =========================================================================
  // Validation (uses commands)
  // =========================================================================

  async validateWorkspace(workspacePath?: string): Promise<any> {
    const path = workspacePath ?? await this.getDefaultWorkspacePath();
    return executeAndExtract('ValidateWorkspace', { path }, 'ValidationResult');
  },

  // =========================================================================
  // File Operations (uses commands except binary)
  // =========================================================================

  async fileExists(path: string): Promise<boolean> {
    return executeAndExtract('FileExists', { path }, 'Bool');
  },

  async readFile(path: string): Promise<string> {
    return executeAndExtract('ReadFile', { path }, 'String');
  },

  async writeFile(path: string, content: string): Promise<void> {
    await executeCommand('WriteFile', { path, content });
  },

  async deleteFile(path: string): Promise<void> {
    await executeCommand('DeleteFile', { path });
  },

  // Binary operations kept as native calls for efficiency (no base64 overhead)
  async readBinary(path: string): Promise<Uint8Array> {
    return getBackend().readBinary(path);
  },

  async writeBinary(path: string, data: Uint8Array): Promise<void> {
    return getBackend().writeBinary(path, data);
  },

  // =========================================================================
  // Export Operations (uses commands)
  // =========================================================================

  async getAvailableAudiences(rootPath: string): Promise<string[]> {
    return executeAndExtract('GetAvailableAudiences', { root_path: rootPath }, 'Strings');
  },

  async planExport(rootPath: string, audience: string): Promise<any> {
    return executeAndExtract('PlanExport', { root_path: rootPath, audience }, 'ExportPlan');
  },

  async exportToMemory(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract('ExportToMemory', { root_path: rootPath, audience }, 'ExportedFiles');
  },

  async exportToHtml(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract('ExportToHtml', { root_path: rootPath, audience }, 'ExportedFiles');
  },

  async exportBinaryAttachments(rootPath: string, audience: string): Promise<{ source_path: string; relative_path: string }[]> {
    return executeAndExtract('ExportBinaryAttachments', { root_path: rootPath, audience }, 'BinaryFilePaths');
  },

  // =========================================================================
  // Sync Client (WasmSyncClient inject/poll bridge)
  // =========================================================================

  /**
   * Create a WasmSyncClient for the given workspace.
   * The client is stored in the worker and accessed via inject/poll methods below.
   */
  createSyncClient(serverUrl: string, workspaceId: string, authToken?: string): void {
    if (syncClient) {
      console.warn('[WasmWorker] Destroying existing sync client');
      syncClient.free?.();
      syncClient = null;
    }
    syncClient = getBackend().createSyncClient(serverUrl, workspaceId, authToken ?? null);
    console.log('[WasmWorker] Created WasmSyncClient for workspace:', workspaceId);
  },

  /**
   * Destroy the sync client.
   */
  destroySyncClient(): void {
    if (syncClient) {
      syncClient.free?.();
      syncClient = null;
      console.log('[WasmWorker] Sync client destroyed');
    }
  },

  /**
   * Get the WebSocket URL from the sync client.
   */
  syncGetWsUrl(): string {
    if (!syncClient) throw new Error('Sync client not created');
    return syncClient.getWsUrl();
  },

  /**
   * Set the session code on the sync client.
   */
  syncSetSessionCode(code: string): void {
    if (!syncClient) throw new Error('Sync client not created');
    syncClient.setSessionCode(code);
  },

  /**
   * Notify the sync client that the WebSocket connected.
   * Returns void — poll outgoing queues and events after this.
   */
  async syncOnConnected(): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.onConnected();
  },

  /**
   * Inject a binary WebSocket message into the sync client.
   */
  async syncOnBinaryMessage(data: Uint8Array): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.onBinaryMessage(data);
  },

  /**
   * Inject a text WebSocket message into the sync client.
   */
  async syncOnTextMessage(text: string): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.onTextMessage(text);
  },

  /**
   * Notify the sync client that the WebSocket disconnected.
   */
  async syncOnDisconnected(): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.onDisconnected();
  },

  /**
   * Notify the sync client that a snapshot was imported.
   */
  async syncOnSnapshotImported(): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.onSnapshotImported();
  },

  /**
   * Queue a local CRDT update for the sync client to send.
   */
  async syncQueueLocalUpdate(docId: string, data: Uint8Array): Promise<void> {
    if (!syncClient) throw new Error('Sync client not created');
    await syncClient.queueLocalUpdate(docId, data);
  },

  /**
   * Drain all outgoing data and events from the sync client.
   * Returns an object with binary messages, text messages, and events.
   * This is more efficient than individual poll calls across the worker boundary.
   */
  syncDrain(): { binary: Uint8Array[]; text: string[]; events: string[] } {
    if (!syncClient) return { binary: [], text: [], events: [] };

    const binary: Uint8Array[] = [];
    const text: string[] = [];
    const events: string[] = [];

    let msg;
    while ((msg = syncClient.pollOutgoingBinary())) {
      binary.push(msg);
    }
    while ((msg = syncClient.pollOutgoingText())) {
      text.push(msg);
    }
    while ((msg = syncClient.pollEvent())) {
      events.push(msg);
    }

    return { binary, text, events };
  },

  /**
   * Send focus messages for specific files.
   */
  syncFocusFiles(files: string[]): void {
    if (!syncClient) return;
    syncClient.focusFiles(files);
  },

  /**
   * Send unfocus messages for specific files.
   */
  syncUnfocusFiles(files: string[]): void {
    if (!syncClient) return;
    syncClient.unfocusFiles(files);
  },

  /**
   * Request body sync for specific files (lazy sync on demand).
   */
  async syncBodyFiles(files: string[]): Promise<void> {
    if (!syncClient) return;
    await syncClient.syncBodyFiles(files);
  },

  // Generic method call for any other operations (fallback to native)
  async call(method: string, args: unknown[]): Promise<unknown> {
    const b = getBackend();
    const fn = (b as any)[method];
    if (typeof fn !== 'function') {
      throw new Error(`Unknown backend method: ${method}`);
    }
    return (fn as Function).apply(b, args);
  },
};

// Expose the worker API via Comlink
Comlink.expose(workerApi);

export type WorkerApi = typeof workerApi;
