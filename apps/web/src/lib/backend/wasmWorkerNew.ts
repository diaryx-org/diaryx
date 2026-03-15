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

import * as Comlink from "comlink";
import type { BackendEventType, BackendEventListener } from "./interface";
import type { StorageType } from "./storageType";

const COMMON_ATTACHMENT_RE =
  /\.(png|jpg|jpeg|gif|svg|pdf|webp|heic|heif|mp3|mp4|wav|mov|docx?|xlsx?|pptx?)$/i;

function isHiddenOrSystemSegment(part: string): boolean {
  return (
    part.startsWith(".") ||
    part === "__MACOSX" ||
    part === "Thumbs.db" ||
    part === "desktop.ini" ||
    part.startsWith("._")
  );
}

function shouldSkipZipPath(path: string): boolean {
  return path.split("/").some((part) => isHiddenOrSystemSegment(part));
}

function detectCommonRootPrefix(fileNames: string[]): string {
  const candidates = fileNames.filter((name) => !shouldSkipZipPath(name));
  if (candidates.length === 0) {
    return "";
  }

  let sharedRoot: string | null = null;
  for (const name of candidates) {
    const firstSlash = name.indexOf("/");
    if (firstSlash <= 0) {
      return "";
    }
    const root = name.substring(0, firstSlash);
    if (sharedRoot === null) {
      sharedRoot = root;
      continue;
    }
    if (sharedRoot !== root) {
      return "";
    }
  }

  return sharedRoot ? `${sharedRoot}/` : "";
}

// We'll dynamically import the WASM module
let backend: any | null = null;

// Discovered workspace root path (set after init)
let rootPath: string | null = null;

// Event port for forwarding filesystem events to main thread
let eventPort: MessagePort | null = null;

// Subscription ID for filesystem events (to clean up on shutdown)
let fsEventSubscriptionId: number | null = null;

// WasmSyncClient instance (created by createSyncClient, lives in worker)

// Clear cached root path (call after rename operations)
function clearRootPathCache() {
  rootPath = null;
}

/**
 * Get the backend instance (throws if not initialized).
 */
function getBackend(): any {
  if (!backend) {
    throw new Error("DiaryxBackend not initialized. Call init() first.");
  }
  return backend;
}

/**
 * Execute a command and parse the response.
 * Helper to avoid repetitive JSON.stringify/parse in each method.
 */
async function executeCommand<T = any>(
  type: string,
  params?: Record<string, any>,
): Promise<T> {
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
  expectedResponseType: string,
): Promise<T> {
  const response = await executeCommand(type, params);
  if (response.type === expectedResponseType) {
    return response.data as T;
  }
  if (response.type === "Ok") {
    return undefined as T; // For void responses
  }
  throw new Error(
    `Expected ${expectedResponseType}, got ${response.type}: ${JSON.stringify(response)}`,
  );
}

let wasmModulePromise: Promise<any> | null = null;

/**
 * Load and initialize the WASM bindings.
 *
 * In dev/main-thread fallback, relying on wasm-pack's implicit `import.meta.url`
 * resolution can fail in some WebKit setups. Prefer an explicit Vite `?url`
 * wasm asset when available, with a safe fallback to the default loader.
 */
async function loadWasmModule(): Promise<any> {
  if (wasmModulePromise) return wasmModulePromise;

  wasmModulePromise = (async () => {
    const wasmCdnUrl = (import.meta as any).env?.VITE_WASM_CDN_URL as
      | string
      | undefined;

    if (wasmCdnUrl) {
      const wasm = await import(/* @vite-ignore */ `${wasmCdnUrl}/diaryx_wasm.js`);
      await wasm.default({ module_or_path: `${wasmCdnUrl}/diaryx_wasm_bg.wasm` });
      return wasm;
    }

    const wasm = await import("$wasm");

    try {
      const wasmUrlModule = await import("$lib/wasm/diaryx_wasm_bg.wasm?url");
      const wasmUrl = (wasmUrlModule as { default?: string }).default;
      if (typeof wasmUrl === "string" && wasmUrl.length > 0) {
        await wasm.default({ module_or_path: wasmUrl });
      } else {
        await wasm.default();
      }
    } catch (e) {
      console.warn(
        "[WasmWorker] Failed to resolve explicit WASM asset URL, using default loader:",
        e,
      );
      await wasm.default();
    }

    return wasm;
  })().catch((e) => {
    wasmModulePromise = null;
    throw e;
  });

  return wasmModulePromise;
}

/**
 * Migrate legacy OPFS workspace directories into the stable workspace ID root.
 * Copies files from legacy name-based roots into the target ID directory.
 */
async function migrateWorkspaceDirectory(
  targetWorkspaceKey: string,
  workspaceName?: string,
): Promise<void> {
  if (targetWorkspaceKey === "diaryx") return;

  try {
    const root = await navigator.storage.getDirectory();

    // Check if target already exists — no migration needed
    try {
      await root.getDirectoryHandle(targetWorkspaceKey, { create: false });
      // Target exists. Still check for legacy .diaryx/ at root to migrate into workspace.
      await migrateLegacyCrdtDir(root, targetWorkspaceKey);
      return;
    } catch {
      // Target doesn't exist, check legacy directories.
    }

    const candidates = Array.from(
      new Set(
        [workspaceName, "diaryx"].filter(
          (value): value is string =>
            !!value && value.length > 0 && value !== targetWorkspaceKey,
        ),
      ),
    );

    for (const candidate of candidates) {
      try {
        const oldDir = await root.getDirectoryHandle(candidate, { create: false });
        console.log(
          `[WasmWorker] Migrating workspace directory: ${candidate} -> ${targetWorkspaceKey}`,
        );
        const newDir = await root.getDirectoryHandle(targetWorkspaceKey, {
          create: true,
        });
        await copyDirectoryRecursive(oldDir, newDir);
        await root.removeEntry(candidate, { recursive: true });
        console.log(
          `[WasmWorker] Migration complete: ${candidate} -> ${targetWorkspaceKey}`,
        );
        await migrateLegacyCrdtDir(root, targetWorkspaceKey);
        return;
      } catch {
        // Candidate doesn't exist — continue scanning.
      }
    }

    await migrateLegacyCrdtDir(root, targetWorkspaceKey);
  } catch (e) {
    console.error("[WasmWorker] Migration failed:", e);
  }
}

/**
 * Migrate legacy ".diaryx/" directory from OPFS root into the workspace directory.
 */
async function migrateLegacyCrdtDir(
  root: FileSystemDirectoryHandle,
  workspaceKey: string,
): Promise<void> {
  try {
    const legacyDir = await root.getDirectoryHandle(".diaryx", {
      create: false,
    });
    // Legacy .diaryx/ exists at root — copy crdt.db into workspace/.diaryx/
    console.log("[WasmWorker] Migrating legacy .diaryx/ into workspace");
    const wsDir = await root.getDirectoryHandle(workspaceKey, {
      create: true,
    });
    const targetDir = await wsDir.getDirectoryHandle(".diaryx", {
      create: true,
    });
    await copyDirectoryRecursive(legacyDir, targetDir);
    await root.removeEntry(".diaryx", { recursive: true });
    console.log("[WasmWorker] Legacy .diaryx/ migrated into workspace");
  } catch {
    // No legacy .diaryx/ at root — nothing to do
  }
}

/**
 * Persist workspace metadata inside the workspace so OPFS discovery does not
 * depend on the directory name.
 */
async function persistWorkspaceMetadata(
  workspaceId: string,
  workspaceName: string,
): Promise<void> {
  try {
    if (!workspaceId) {
      return;
    }
    await executeCommand("WriteFile", {
      path: ".diaryx/workspace.json",
      content: JSON.stringify(
        {
          id: workspaceId,
          name: workspaceName,
        },
        null,
        2,
      ),
    });
  } catch (e) {
    console.warn("[WasmWorker] Failed to persist workspace metadata:", e);
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
    if (handle.kind === "file") {
      const file = await (handle as FileSystemFileHandle).getFile();
      const data = await file.arrayBuffer();
      const newFile = await dest.getFileHandle(name, { create: true });
      const writable = await newFile.createWritable();
      await writable.write(data);
      await writable.close();
    } else if (handle.kind === "directory") {
      const newSubDir = await dest.getDirectoryHandle(name, { create: true });
      await copyDirectoryRecursive(
        handle as FileSystemDirectoryHandle,
        newSubDir,
      );
    }
  }
}

/**
 * Handle a SnapshotDownloaded event from Rust.
 * Imports the zip blob into the backend, then notifies Rust that import is done.
 */
async function handleSnapshotDownloaded(blob: Blob, workspaceId: string): Promise<void> {
  try {
    console.log(`[WasmWorker] Snapshot downloaded for ${workspaceId}, importing...`);
    const file = new File([blob], "snapshot.zip", { type: "application/zip" });
    const b = getBackend();
    await workerApi.importFromZip(file);
    // Notify Rust that the snapshot was imported so the handshake can continue
    if (b.notifySnapshotImported) {
      b.notifySnapshotImported();
    }
    console.log(`[WasmWorker] Snapshot import complete for ${workspaceId}`);
  } catch (e) {
    console.error("[WasmWorker] Failed to import snapshot:", e);
    // Still notify so the handshake doesn't hang
    try {
      const b = getBackend();
      if (b.notifySnapshotImported) {
        b.notifySnapshotImported();
      }
    } catch { /* ignore */ }
  }
}

/**
 * Initialize the backend and set up event forwarding.
 */
async function init(
  port: MessagePort,
  storageType: StorageType,
  workspaceName?: string,
  directoryHandle?: FileSystemDirectoryHandle,
  workspaceId?: string,
): Promise<void> {
  // Store event port for forwarding filesystem events
  eventPort = port;

  const resolvedWorkspaceId = workspaceId || workspaceName || "My Journal";
  const resolvedWorkspaceName = workspaceName || "My Journal";

  // For OPFS, run legacy directory migrations into the stable workspace ID root.
  if (storageType === "opfs") {
    await migrateWorkspaceDirectory(resolvedWorkspaceId, resolvedWorkspaceName);
  }

  // CRDT storage bridge setup was removed from the web host. Sync/CRDT state
  // is plugin-owned; the host only initializes backend storage + command runtime.

  const wasm = await loadWasmModule();

  // Create backend with specified storage type
  if (storageType === "opfs") {
    try {
      backend = await wasm.DiaryxBackend.createOpfs(resolvedWorkspaceId);
    } catch (e) {
      console.warn("[WasmWorker] OPFS unavailable, falling back to IndexedDB:", e);
      backend = await wasm.DiaryxBackend.createIndexedDb(resolvedWorkspaceId);
    }
  } else if (storageType === "filesystem-access") {
    if (!directoryHandle) {
      throw new Error(
        "Directory handle required for filesystem-access storage type",
      );
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createFromDirectoryHandle(
      directoryHandle,
    );
  } else if (storageType === "memory") {
    // In-memory storage for guest mode - files live only in memory
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    backend = (wasm.DiaryxBackend as any).createInMemory();
  } else {
    backend = await wasm.DiaryxBackend.createIndexedDb(
      resolvedWorkspaceId,
    );
  }

  await persistWorkspaceMetadata(resolvedWorkspaceId, resolvedWorkspaceName);

  // Subscribe to filesystem events and forward them to the main thread.
  // The callback receives either a JSON string (FileSystemEvent) or a raw
  // JS object (e.g. SnapshotDownloaded with a Blob).
  if (backend.onFileSystemEvent && eventPort) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    fsEventSubscriptionId = backend.onFileSystemEvent((eventJson: any) => {
      // Raw JS object events (e.g. SnapshotDownloaded) are objects, not strings
      if (typeof eventJson === "object" && eventJson?.type === "SnapshotDownloaded") {
        handleSnapshotDownloaded(eventJson.blob, eventJson.workspace_id);
        return;
      }
      // Forward the event JSON to the main thread via MessagePort
      try {
        eventPort!.postMessage({ type: "FileSystemEvent", data: eventJson });
      } catch (e) {
        console.error("[WasmWorker] Failed to forward filesystem event:", e);
      }
    });
    console.log(
      "[WasmWorker] Subscribed to filesystem events, id:",
      fsEventSubscriptionId,
    );
  } else {
    console.log("[WasmWorker] Filesystem events not available on this backend");
  }

  console.log(
    "[WasmWorker] DiaryxBackend initialized with storage:",
    storageType,
  );
}

/**
 * Initialize the backend with a File System Access API directory handle.
 * This is called when the user selects "Local Folder" storage.
 */
async function initWithDirectoryHandle(
  _port: MessagePort,
  directoryHandle: FileSystemDirectoryHandle,
): Promise<void> {
  return init(
    _port,
    "filesystem-access",
    undefined,
    directoryHandle,
  );
}

/**
 * Worker API exposed via Comlink.
 *
 * All methods use the unified command API through `execute()` except where noted.
 */

export const workerApi = {
  init,
  initWithDirectoryHandle,

  /**
   * Legacy no-op kept for compatibility with older callers.
   * Web host no longer initializes a CRDT storage bridge.
   */
  async setupCrdtStorage(): Promise<void> {
    // Intentionally empty.
  },

  isReady(): boolean {
    return backend !== null;
  },

  // Event stubs - events will go through MessagePort
  on(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn("[WasmWorker] Events are forwarded via MessagePort, not on()");
  },
  off(_event: BackendEventType, _listener: BackendEventListener): void {
    console.warn(
      "[WasmWorker] Events are forwarded via MessagePort, not off()",
    );
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
    const directory = dirPath ?? ".";
    const response = await executeCommand("FindRootIndex", { directory });
    if (response.type === "String") {
      return response.data;
    }
    return null;
  },

  async getDefaultWorkspacePath(): Promise<string> {
    // Return cached path if available
    if (rootPath) return rootPath;

    // Try to discover root index in current directory first
    let root = await this.findRootIndex(".");

    // Fallback: try "workspace" directory (OPFS default)
    if (!root) {
      root = await this.findRootIndex("workspace");
    }

    // Fallback: scan all top-level directories for a root index
    if (!root) {
      try {
        // Use GetFilesystemTree to list directories
        const response = await executeCommand("GetFilesystemTree", {
          path: ".",
          show_hidden: false,
          depth: 1,
        });
        if (response.type === "Tree" && response.data?.children) {
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
        console.warn("[WasmWorker] Failed to scan directories:", e);
      }
    }

    if (root) {
      // Get parent directory of root index
      const lastSlash = root.lastIndexOf("/");
      const discoveredPath = lastSlash > 0 ? root.substring(0, lastSlash) : ".";
      rootPath = discoveredPath;
      return discoveredPath;
    }

    // Fallback to current directory
    return ".";
  },

  // Clear cached root path (for after rename operations)
  clearRootPathCache(): void {
    clearRootPathCache();
  },

  // =========================================================================
  // Workspace (uses commands)
  // =========================================================================

  async getWorkspaceTree(
    workspacePath?: string,
    depth?: number,
    audience?: string,
  ): Promise<any> {
    const path = workspacePath ?? (await this.getDefaultWorkspacePath());
    return executeAndExtract(
      "GetWorkspaceTree",
      { path, depth: depth ?? null, audience: audience ?? null },
      "Tree",
    );
  },

  async createWorkspace(path?: string, name?: string): Promise<string> {
    const workspacePath = path ?? ".";
    const workspaceName = name ?? "My Workspace";
    await executeCommand("CreateWorkspace", {
      path: workspacePath,
      name: workspaceName,
    });
    rootPath = workspacePath; // Cache the new workspace path
    return workspacePath;
  },

  async getFilesystemTree(
    workspacePath?: string,
    showHidden?: boolean,
  ): Promise<any> {
    const path = workspacePath ?? (await this.getDefaultWorkspacePath());
    return executeAndExtract(
      "GetFilesystemTree",
      {
        path,
        show_hidden: showHidden ?? false,
        depth: null,
      },
      "Tree",
    );
  },

  // =========================================================================
  // Entries (uses commands)
  // =========================================================================

  async getEntry(path: string): Promise<any> {
    return executeAndExtract("GetEntry", { path }, "Entry");
  },

  async saveEntry(path: string, content: string): Promise<void> {
    await executeCommand("SaveEntry", { path, content });
  },

  async createEntry(
    path: string,
    options?: { title?: string },
  ): Promise<string> {
    return executeAndExtract(
      "CreateEntry",
      {
        path,
        options: {
          title: options?.title ?? null,
          part_of: null,
          template: null,
        },
      },
      "String",
    );
  },

  async deleteEntry(path: string): Promise<void> {
    await executeCommand("DeleteEntry", { path });
  },

  async moveEntry(fromPath: string, toPath: string): Promise<string> {
    await executeCommand("MoveEntry", { from: fromPath, to: toPath });
    return toPath;
  },

  async renameEntry(path: string, newFilename: string): Promise<string> {
    const result = await executeAndExtract<string>(
      "RenameEntry",
      { path, new_filename: newFilename },
      "String",
    );
    // Clear cached root path in case we renamed the root index
    clearRootPathCache();
    return result;
  },

  async duplicateEntry(path: string): Promise<string> {
    return executeAndExtract("DuplicateEntry", { path }, "String");
  },

  // =========================================================================
  // Frontmatter (uses commands)
  // =========================================================================

  async getFrontmatter(path: string): Promise<any> {
    return executeAndExtract("GetFrontmatter", { path }, "Frontmatter");
  },

  async setFrontmatterProperty(
    path: string,
    key: string,
    value: any,
  ): Promise<void> {
    await executeCommand("SetFrontmatterProperty", { path, key, value });
  },

  // =========================================================================
  // Search (uses commands)
  // =========================================================================

  async searchWorkspace(pattern: string, options?: any): Promise<any> {
    const workspacePath =
      options?.workspacePath ?? (await this.getDefaultWorkspacePath());
    return executeAndExtract(
      "SearchWorkspace",
      {
        pattern,
        options: {
          workspace_path: workspacePath,
          search_frontmatter: options?.searchFrontmatter ?? false,
          property: options?.property ?? null,
          case_sensitive: options?.caseSensitive ?? false,
        },
      },
      "SearchResults",
    );
  },

  // =========================================================================
  // Validation (uses commands)
  // =========================================================================

  async validateWorkspace(workspacePath?: string): Promise<any> {
    const path = workspacePath ?? (await this.getDefaultWorkspacePath());
    return executeAndExtract("ValidateWorkspace", { path }, "ValidationResult");
  },

  // =========================================================================
  // File Operations (uses commands except binary)
  // =========================================================================

  async fileExists(path: string): Promise<boolean> {
    return executeAndExtract("FileExists", { path }, "Bool");
  },

  async readFile(path: string): Promise<string> {
    return executeAndExtract("ReadFile", { path }, "String");
  },

  async writeFile(path: string, content: string): Promise<void> {
    await executeCommand("WriteFile", { path, content });
  },

  async deleteFile(path: string): Promise<void> {
    await executeCommand("DeleteFile", { path });
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
    return executeAndExtract(
      "GetAvailableAudiences",
      { root_path: rootPath },
      "Strings",
    );
  },

  async planExport(rootPath: string, audience: string): Promise<any> {
    return executeAndExtract(
      "PlanExport",
      { root_path: rootPath, audience },
      "ExportPlan",
    );
  },

  async exportToMemory(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract(
      "ExportToMemory",
      { root_path: rootPath, audience },
      "ExportedFiles",
    );
  },

  async exportToHtml(rootPath: string, audience: string): Promise<any[]> {
    return executeAndExtract(
      "ExportToHtml",
      { root_path: rootPath, audience },
      "ExportedFiles",
    );
  },

  async exportBinaryAttachments(
    rootPath: string,
    audience: string,
  ): Promise<{ source_path: string; relative_path: string }[]> {
    return executeAndExtract(
      "ExportBinaryAttachments",
      { root_path: rootPath, audience },
      "BinaryFilePaths",
    );
  },

  // =========================================================================
  // Rust-Owned Sync (Rust owns the WebSocket)
  // =========================================================================

  /**
   * Start sync — Rust owns the WebSocket connection.
   * Creates a WasmSyncTransport, connects to the server, and subscribes
   * to local CRDT updates. All sync events are forwarded via the filesystem
   * event port (MessagePort).
   */
  startSync(
    serverUrl: string,
    workspaceId: string,
    authToken?: string,
    sessionCode?: string,
  ): void {
    getBackend().startSync(
      serverUrl,
      workspaceId,
      authToken ?? null,
      sessionCode ?? null,
    );
    console.log(
      "[WasmWorker] Started Rust-owned sync for workspace:",
      workspaceId,
    );
  },

  /**
   * Stop sync — disconnect and drop the transport.
   */
  stopSync(): void {
    getBackend().stopSync();
    console.log("[WasmWorker] Stopped Rust-owned sync");
  },

  /**
   * Focus on specific files for body sync (Rust-owned).
   */
  focusSyncFiles(files: string[]): void {
    getBackend().focusSyncFiles(files);
  },

  /**
   * Unfocus specific files (Rust-owned).
   */
  unfocusSyncFiles(files: string[]): void {
    getBackend().unfocusSyncFiles(files);
  },

  /**
   * Request body sync for specific files (Rust-owned).
   */
  requestBodySync(files: string[]): void {
    getBackend().requestBodySync(files);
  },

  /**
   * Notify Rust that a snapshot import has completed.
   */
  notifySnapshotImported(): void {
    const b = getBackend();
    if (b.notifySnapshotImported) {
      b.notifySnapshotImported();
    }
  },

  // =========================================================================
  // Attachment Sync (Rust-backed)
  // =========================================================================

  async syncUploadAttachment(
    serverUrl: string, authToken: string, workspaceId: string,
    entryPath: string, attachmentPath: string, hash: string,
    mimeType: string, data: Uint8Array,
  ): Promise<void> {
    return getBackend().uploadAttachment(
      serverUrl, authToken, workspaceId,
      entryPath, attachmentPath, hash,
      mimeType, Array.from(data),
    );
  },

  async syncDownloadAttachment(
    serverUrl: string, authToken: string, workspaceId: string, hash: string,
  ): Promise<Uint8Array> {
    const bytes = await getBackend().downloadAttachment(serverUrl, authToken, workspaceId, hash);
    return new Uint8Array(bytes);
  },

  // =========================================================================
  // ZIP Import (runs entirely in worker — no main-thread decompression)
  // =========================================================================

  async importFromZip(
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<{
    success: boolean;
    files_imported: number;
    files_skipped: number;
  }> {
    const { ZipReader, BlobReader, TextWriter, Uint8ArrayWriter } =
      await import("@zip.js/zip.js");

    let workspace: string;
    if (workspacePath) {
      workspace = workspacePath;
    } else {
      try {
        workspace = await workerApi.getDefaultWorkspacePath();
      } catch {
        workspace = ".";
      }
    }

    const zipReader = new ZipReader(new BlobReader(file));

    try {
      const entries = await zipReader.getEntries();
      const files = entries.filter((entry) => !entry.directory);
      const commonPrefix = detectCommonRootPrefix(
        files.map((entry) => entry.filename),
      );

      let filesImported = 0;
      let filesSkipped = 0;
      let processedWeight = 0;
      const now = () =>
        typeof performance !== "undefined" &&
        typeof performance.now === "function"
          ? performance.now()
          : Date.now();
      let lastProgressEmitAt = now();

      const entryWeights = files.map((entry) => {
        const sizeGuess = entry.uncompressedSize || entry.compressedSize || 0;
        return sizeGuess > 0 ? sizeGuess : 1;
      });
      const totalWeight = entryWeights.reduce((sum, weight) => sum + weight, 0);

      for (let i = 0; i < files.length; i++) {
        const entry = files[i];
        let fileName = entry.filename;

        if (commonPrefix && fileName.startsWith(commonPrefix)) {
          fileName = fileName.substring(commonPrefix.length);
          if (fileName === "") {
            continue;
          }
        }

        if (shouldSkipZipPath(fileName)) {
          filesSkipped++;
          continue;
        }

        const isMarkdown = fileName.endsWith(".md");
        const isAttachment = COMMON_ATTACHMENT_RE.test(fileName);
        if (!isMarkdown && !isAttachment) {
          filesSkipped++;
          continue;
        }

        const filePath = `${workspace}/${fileName}`;
        try {
          if (isMarkdown) {
            const content = await entry.getData!(new TextWriter());
            await workerApi.writeFile(filePath, content as string);
          } else {
            const data = await entry.getData!(new Uint8ArrayWriter());
            await workerApi.writeBinary(filePath, data as Uint8Array);
          }
          filesImported++;
        } catch (e) {
          filesSkipped++;
          console.warn(`[Import] Failed to import ${filePath}:`, e);
        }

        if (onProgress && totalWeight > 0) {
          processedWeight = Math.min(
            totalWeight,
            processedWeight + entryWeights[i],
          );
          const tick = now();
          const shouldEmit =
            processedWeight >= totalWeight || tick - lastProgressEmitAt >= 120;
          if (shouldEmit) {
            onProgress(processedWeight, totalWeight);
            lastProgressEmitAt = tick;
          }
        }
      }

      if (onProgress) {
        onProgress(totalWeight, totalWeight);
      }

      return {
        success: true,
        files_imported: filesImported,
        files_skipped: filesSkipped,
      };
    } finally {
      await zipReader.close();
    }
  },

  // Generic method call for any other operations (fallback to native)
  async call(method: string, args: unknown[]): Promise<unknown> {
    const b = getBackend();
    const fn = (b as any)[method];
    if (typeof fn !== "function") {
      throw new Error(`Unknown backend method: ${method}`);
    }
    return (fn as Function).apply(b, args);
  },
};

// Expose the worker API via Comlink only when running inside a real Worker.
const isWorkerRuntime =
  typeof self !== "undefined" &&
  typeof (self as { importScripts?: unknown }).importScripts === "function";

if (isWorkerRuntime) {
  Comlink.expose(workerApi);
}

export type WorkerApi = typeof workerApi;
