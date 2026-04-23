// Backend interface - abstracts over Tauri IPC and WASM implementations

// Import generated types from Rust
import type { Command, Response } from './generated';

// Re-export generated types for consumers.
//
// The data shapes below all come from Rust via ts-rs — DO NOT hand-maintain
// duplicates here. Add new types to `crates/diaryx_core/src/command.rs`
// (or the appropriate crate), then run `cargo xtask sync-bindings`.
export type { Command, Response } from './generated';
export type {
  EntryData,
  TreeNode,
  SearchMatch,
  FileSearchResult,
  SearchResults,
  ValidationResult,
  ValidationResultWithMeta,
  ValidationError,
  ValidationErrorWithMeta,
  ValidationWarning,
  ValidationWarningWithMeta,
  FixResult,
  FixSummary,
  ExportedFile,
  BinaryExportFile,
} from './generated';

// Back-compat aliases (from when we re-exported generated types under a
// different name). Prefer the unprefixed names above; these remain so
// existing imports continue to resolve during the TS→Rust migration.
export type {
  EntryData as GeneratedEntryData,
  TreeNode as GeneratedTreeNode,
  SearchResults as GeneratedSearchResults,
  ValidationResult as GeneratedValidationResult,
  FixResult as GeneratedFixResult,
  ExportPlan as GeneratedExportPlan,
} from './generated';

// ============================================================================
// Host-only types (no Rust equivalent yet — these live only in the web host)
// ============================================================================

export interface Config {
  default_workspace: string;
  editor?: string;
}

export interface AppUpdateInfo {
  version: string;
  body: string | null;
}

// ============================================================================
// Legacy host-side types that diverge from the generated Rust equivalents.
// These should eventually be reconciled — each is flagged below with the
// specific drift vs. Rust. DO NOT add new fields here; extend the Rust
// source of truth instead.
// ============================================================================

/**
 * Simplified export plan used by the host's in-TS planner.
 *
 * Diverges from the generated `ExportPlan` (which uses `ExportFile` /
 * `ExcludedFile` / `ExclusionReason` with richer structure). See
 * `api.planExport` — that function builds the simple host shape directly
 * from `getWorkspaceTree`, rather than round-tripping through Rust.
 *
 * TODO(migration): replace with generated `ExportPlan` when the planner
 * moves into Rust.
 */
export interface ExportPlan {
  included: { path: string; relative_path: string }[];
  excluded: { path: string; reason: string }[];
  audience: string;
}

/**
 * Host-side storage info shape (uses `number` for sizes).
 *
 * Diverges from generated `StorageInfo` which uses `bigint` for u64 fields.
 * TODO(migration): switch callers to the generated type after auditing
 * every arithmetic site (sizes fit comfortably in Number for the
 * foreseeable future).
 */
export interface StorageInfo {
  used: number;
  limit: number;
  attachment_limit: number;
}

// Backup types
export interface BackupStatus {
  target_name: string;
  success: boolean;
  files_processed: number;
  error?: string;
}

export interface BackupData {
  text_files: [string, string][];
  binary_files: { path: string; data: number[] }[];
  text_count: number;
  binary_count: number;
}

export interface SearchOptions {
  workspacePath?: string;
  searchFrontmatter?: boolean;
  property?: string;
  caseSensitive?: boolean;
}

export interface CreateEntryOptions {
  title?: string | null;
  partOf?: string | null;
  template?: string | null;
}

export interface TemplateInfo {
  name: string;
  path: string;
  source: "workspace" | "user" | "builtin";
}

// Import types
export interface ImportResult {
  success: boolean;
  files_imported: number;
  error?: string;
}

export interface PluginInspection {
  pluginId: string;
  pluginName: string;
  requestedPermissions?: unknown;
}

// ============================================================================
// Backend Events
// ============================================================================

/**
 * Events emitted by Backend operations.
 * Subscribe to these to automatically update CRDT state.
 */
export type BackendEventType =
  | 'file:created'
  | 'file:deleted'
  | 'file:renamed'
  | 'file:moved'
  | 'metadata:changed'
  | 'contents:changed';

export interface FileCreatedEvent {
  type: 'file:created';
  path: string;
  frontmatter: Record<string, unknown>;
  parentPath?: string;
}

export interface FileDeletedEvent {
  type: 'file:deleted';
  path: string;
  parentPath?: string;
}

export interface FileRenamedEvent {
  type: 'file:renamed';
  oldPath: string;
  newPath: string;
}

export interface FileMovedEvent {
  type: 'file:moved';
  path: string;
  oldParent?: string;
  newParent?: string;
}

export interface MetadataChangedEvent {
  type: 'metadata:changed';
  path: string;
  frontmatter: Record<string, unknown>;
}

export interface ContentsChangedEvent {
  type: 'contents:changed';
  path: string;
  contents: string[];
}

export type BackendEvent =
  | FileCreatedEvent
  | FileDeletedEvent
  | FileRenamedEvent
  | FileMovedEvent
  | MetadataChangedEvent
  | ContentsChangedEvent;

export type BackendEventListener = (event: BackendEvent) => void;

/**
 * FileSystemEvent from the Rust backend.
 * These events are emitted by the decorated filesystem layer.
 */
export type FileSystemEvent =
  | { type: 'FileCreated'; path: string; frontmatter?: unknown; parent_path?: string }
  | { type: 'FileDeleted'; path: string; parent_path?: string }
  | { type: 'FileRenamed'; old_path: string; new_path: string }
  | { type: 'FileMoved'; path: string; old_parent?: string; new_parent?: string }
  | { type: 'MetadataChanged'; path: string; frontmatter: unknown }
  | { type: 'ContentsChanged'; path: string; body: string }
  // Sync events
  | { type: 'SyncStarted'; doc_name: string }
  | { type: 'SyncCompleted'; doc_name: string; files_synced: number }
  | { type: 'SyncStatusChanged'; status: string; error?: string }
  | {
      type: 'SyncProgress';
      completed: number;
      total: number;
      percent?: number;
      phase?: string;
      message?: string;
      path?: string;
    }
  // Send sync message event - emitted by Rust after CRDT updates
  | { type: 'SendSyncMessage'; doc_name: string; message: number[]; is_body: boolean }
  // Peer/session events from Rust sync
  | { type: 'PeerJoined'; peer_count: number }
  | { type: 'PeerLeft'; peer_count: number }
  | { type: 'FocusListChanged'; files: string[] };

/**
 * Callback type for filesystem event subscriptions.
 */
export type FileSystemEventCallback = (event: FileSystemEvent) => void;


// ============================================================================
// Backend Interface
// ============================================================================

/**
 * Backend interface that abstracts over different runtime environments.
 *
 * - TauriBackend: Uses Tauri IPC to communicate with the Rust backend
 * - WasmBackend: Uses WebAssembly module with InMemoryFileSystem + IndexedDB
 *
 * ## API: execute()
 *
 * All operations go through the `execute()` method with typed Command objects.
 * Use the `api.ts` wrapper for ergonomic typed access.
 *
 * @example
 * ```ts
 * // Preferred: Use api wrapper
 * import { createApi } from './api';
 * const api = createApi(backend);
 * const entry = await api.getEntry('workspace/notes.md');
 *
 * // Or use execute() directly
 * const response = await backend.execute({
 *   type: 'GetEntry',
 *   params: { path: 'workspace/notes.md' }
 * });
 * ```
 */
export interface Backend {
  /**
   * Initialize the backend. Must be called before any other methods.
   * For WASM, this loads data from IndexedDB into the InMemoryFileSystem.
   * For Tauri, this is a no-op.
   *
   * @param storageTypeOverride Optional storage type to use instead of the default.
   *                            Use 'memory' for guest mode (in-memory filesystem).
   */
  init(storageTypeOverride?: string, workspaceId?: string, workspaceName?: string, storagePluginId?: string): Promise<void>;

  /**
   * Check if the backend is ready to use.
   */
  isReady(): boolean;

  /**
   * Get the default workspace path for this backend.
   * For Tauri: Returns the path from config/platform (e.g., ~/diaryx)
   * For WASM: Returns "workspace" (virtual path in IndexedDB/OPFS)
   */
  getWorkspacePath(): string;

  /**
   * Get the config for this backend (if available).
   * For Tauri: Returns the config loaded from disk.
   * For WASM: Returns null (config not applicable).
   */
  getConfig(): Config | null;

  /**
   * Get app paths (Tauri-specific, returns null for WASM).
   * Includes data_dir, document_dir, default_workspace, config_path, is_mobile,
   * and is_apple_build when available.
   */
  getAppPaths(): Record<string, string | boolean | null> | null;

  // --------------------------------------------------------------------------
  // CrdtFs Control
  // --------------------------------------------------------------------------

  /**
   * Enable or disable CrdtFs (CRDT updates on file writes).
   * CrdtFs starts disabled and should be enabled after sync handshake completes.
   */
  setCrdtEnabled?(enabled: boolean): Promise<void>;

  /**
   * Check whether CrdtFs is currently enabled.
   */
  isCrdtEnabled?(): Promise<boolean>;

  /**
   * Enter an isolated guest workspace mode.
   * Hosts may swap to an in-memory backend or otherwise isolate local writes.
   */
  startGuestMode?(sessionCode: string): Promise<void>;

  /**
   * Leave guest workspace mode and restore the prior workspace/backend.
   */
  endGuestMode?(): Promise<void>;

  /**
   * Check whether the backend is currently operating in guest mode.
   */
  isGuestMode?(): Promise<boolean>;

  // --------------------------------------------------------------------------
  // Unified Command API
  // --------------------------------------------------------------------------

  /**
   * Execute a command and return the response.
   *
   * **This is the primary API.** All operations should use this method.
   * The typed wrapper in `api.ts` provides ergonomic access to this method.
   *
   * @example
   * ```ts
   * const response = await backend.execute({
   *   type: 'GetEntry',
   *   params: { path: 'workspace/notes.md' }
   * });
   * if (response.type === 'Entry') {
   *   console.log(response.data.title);
   * }
   * ```
   */
  execute(command: Command): Promise<Response>;

  // --------------------------------------------------------------------------
  // Events
  // --------------------------------------------------------------------------

  /**
   * Subscribe to backend events.
   * Use this to automatically update CRDT state when files change.
   */
  on(event: BackendEventType, listener: BackendEventListener): void;

  /**
   * Unsubscribe from backend events.
   */
  off(event: BackendEventType, listener: BackendEventListener): void;

  // --------------------------------------------------------------------------
  // Platform-specific methods
  // --------------------------------------------------------------------------

  /**
   * Persist any pending changes to storage.
   * For WASM: writes InMemoryFileSystem contents to IndexedDB.
   * For Tauri: no-op (changes are written directly to disk).
   */
  persist(): Promise<void>;

  /**
   * Read a binary file's content.
   * Uses native Uint8Array transfer (no JSON/base64 overhead).
   */
  readBinary(path: string): Promise<Uint8Array>;

  /**
   * Write binary content to a file.
   * Uses native Uint8Array transfer (no JSON/base64 overhead).
   */
  writeBinary(path: string, data: Uint8Array): Promise<void>;

  /**
   * Reveal a workspace item in the system file manager when supported.
   * Available in Tauri desktop builds.
   */
  revealInFileManager?(path: string): Promise<void>;

  /**
   * Read the active application log file when supported.
   * Available in Tauri builds that expose a native log file.
   */
  readLogFile?(): Promise<string>;

  /**
   * Check for a direct-distribution desktop app update when supported.
   * Returns null when the updater is unavailable or no update is published.
   */
  checkForAppUpdate?(): Promise<AppUpdateInfo | null>;

  /**
   * Download and install the latest direct-distribution desktop app update.
   * Returns false when the updater is unavailable or there is nothing to install.
   */
  installAppUpdate?(): Promise<boolean>;

  /**
   * Import workspace from a zip file.
   * Handles large files by streaming in chunks.
   * @param file The File object from a file input.
   * @param workspacePath Optional workspace path to import into.
   * @param onProgress Optional callback for progress updates.
   * @returns Import result with success status and file count.
   *
   * Note: This requires the browser File API.
   */
  importFromZip(
    file: File,
    workspacePath?: string,
    onProgress?: (bytesUploaded: number, totalBytes: number) => void,
  ): Promise<ImportResult>;

  // --------------------------------------------------------------------------
  // Filesystem Event Subscription (from Rust decorator layer)
  // --------------------------------------------------------------------------

  /**
   * Subscribe to filesystem events from the Rust decorator layer.
   *
   * The callback receives FileSystemEvent objects when filesystem operations
   * occur (create, delete, rename, move, metadata changes, etc.).
   *
   * @param callback Function called with each filesystem event
   * @returns Subscription ID that can be used to unsubscribe
   *
   * @example
   * ```ts
   * const id = backend.onFileSystemEvent((event) => {
   *   if (event.type === 'FileCreated') {
   *     console.log('New file:', event.path);
   *   }
   * });
   *
   * // Later, to unsubscribe:
   * backend.offFileSystemEvent(id);
   * ```
   */
  onFileSystemEvent?(callback: FileSystemEventCallback): number;

  /**
   * Unsubscribe from filesystem events.
   *
   * @param id The subscription ID returned by onFileSystemEvent
   * @returns true if the subscription was found and removed
   */
  offFileSystemEvent?(id: number): boolean;

  /**
   * Register a callback for when background plugin loading completes.
   * Returns an unsubscribe function.
   */
  onPluginsReady?(callback: () => void): () => void;

  /**
   * Register a callback that fires once per plugin as it finishes init,
   * in completion order (not registration order). Useful for streaming
   * UI updates: a slow plugin no longer blocks others from being shown.
   *
   * `gen` is a per-load generation tag — if a workspace switch races with
   * an in-flight load, listeners can compare `gen` to ignore stale events.
   *
   * Returns an unsubscribe function.
   */
  onPluginReady?(
    callback: (event: { id: string; ok: boolean; error?: string | null; gen: number }) => void,
  ): () => void;

  /**
   * Manually emit a filesystem event.
   *
   * Primarily used for testing or manual sync scenarios.
   *
   * @param event The event to emit
   */
  emitFileSystemEvent?(event: FileSystemEvent): void;

  /**
   * Get the number of active event subscriptions.
   */
  eventSubscriberCount?(): number;

  // --------------------------------------------------------------------------
  // Native Sync (Tauri only)
  // --------------------------------------------------------------------------

  /**
   * Start native WebSocket sync to a server.
   * Available in Tauri (native Rust sync client) and WASM (Rust-owned WebSocket).
   *
   * @param serverUrl The WebSocket server URL (will be converted to ws:// if http://)
   * @param docNameOrWorkspaceId The document name (Tauri) or workspace ID (WASM) for sync
   * @param authToken Optional JWT auth token
   * @param sessionCode Optional share session code (WASM only)
   */
  startSync?(serverUrl: string, docNameOrWorkspaceId: string, authToken?: string, sessionCode?: string): Promise<void>;

  /**
   * Stop WebSocket sync.
   * Available in Tauri and WASM.
   */
  stopSync?(): Promise<void>;

  /**
   * Get native sync status.
   * Only available in Tauri.
   *
   * @returns Sync status with connected, running, and detailed status info
   */
  getSyncStatus?(): Promise<SyncStatus>;

  /**
   * Check if native sync is available.
   * Returns true for Tauri, false for WASM/web.
   */
  hasNativeSync?(): boolean;

  /**
   * Subscribe to native sync events.
   * Only available in Tauri.
   *
   * @param callback Function called with each sync event
   * @returns Unsubscribe function
   */
  onSyncEvent?(callback: SyncEventCallback): () => void;

  /**
   * Focus on specific files for body sync.
   * Available in Tauri (native Rust sync).
   * For browser-loaded plugins, body sync is handled by the Extism host bridge.
   */
  focusSyncFiles?(files: string[]): Promise<void>;

  /**
   * Request body sync for specific files.
   * Available in Tauri (native Rust sync).
   * For browser-loaded plugins, body sync is handled by the Extism host bridge.
   */
  requestBodySync?(files: string[]): Promise<void>;

  // =========================================================================
  // Plugin Management (Tauri only)
  // =========================================================================

  /** Install a user plugin from WASM bytes. Returns the manifest JSON string. */
  installPlugin?(wasmBytes: Uint8Array): Promise<string>;
  /** Inspect a user plugin from WASM bytes without installing it. */
  inspectPlugin?(wasmBytes: Uint8Array): Promise<PluginInspection>;
  /** Uninstall a user plugin by ID. */
  uninstallPlugin?(pluginId: string): Promise<void>;
  /** Fetch raw HTML for a plugin-owned iframe component. */
  getPluginComponentHtml?(pluginId: string, componentId: string): Promise<string>;
  /** Execute a plugin command with temporary host-provided files. */
  executePluginCommandWithFiles?(
    pluginId: string,
    command: string,
    params: unknown,
    requestFiles: Record<string, Uint8Array>,
  ): Promise<unknown>;

}

// ============================================================================
// Sync Types
// ============================================================================

/**
 * Sync status returned by getSyncStatus().
 */
export interface SyncStatus {
  /** Whether both metadata and body connections are established */
  connected: boolean;
  /** Whether the sync client is running (may be reconnecting) */
  running: boolean;
  /** Detailed connection status */
  status?: ConnectionStatus;
}

/**
 * Detailed connection status for sync.
 */
export interface ConnectionStatus {
  /** Metadata WebSocket connection state */
  metadata: 'disconnected' | 'connecting' | 'connected';
  /** Body sync WebSocket connection state */
  body: 'disconnected' | 'connecting' | 'connected';
}

/**
 * Sync events emitted by native sync.
 */
export type SyncEvent =
  | { type: 'status-changed'; status: ConnectionStatus }
  | { type: 'files-changed'; paths: string[] }
  | { type: 'body-changed'; path: string }
  | { type: 'progress'; completed: number; total: number }
  | { type: 'error'; message: string };

/**
 * Callback type for sync event subscriptions.
 */
export type SyncEventCallback = (event: SyncEvent) => void;

// ============================================================================
// Error Types
// ============================================================================

export class BackendError extends Error {
  constructor(
    message: string,
    public readonly kind: string,
    public readonly path?: string,
  ) {
    super(message);
    this.name = "BackendError";
  }
}

// ============================================================================
// Runtime Detection
// ============================================================================

/**
 * Check if running in a Tauri environment.
 */
export function isTauri(): boolean {
  if (typeof window === "undefined") {
    return false;
  }

  const runtime = globalThis as typeof globalThis & {
    isTauri?: boolean;
    __TAURI_INTERNALS__?: unknown;
  };

  return runtime.isTauri === true || typeof runtime.__TAURI_INTERNALS__ === "object";
}

/**
 * Check if running with the HTTP backend (`diaryx edit`).
 *
 * Like Tauri, the HTTP backend runs commands natively (via the CLI binary)
 * rather than in-browser WASM, so plugin loading should follow the native
 * path rather than the browser Extism/JSPI path.
 */
export function isHttpBackend(): boolean {
  if (typeof window === "undefined") return false;
  const params = new URLSearchParams(window.location.search);
  return params.get("backend") === "http" && !!params.get("api_url");
}

/**
 * Get the HTTP backend API URL, if using the HTTP backend.
 * Returns `null` when not in HTTP backend mode.
 */
export function getHttpApiUrl(): string | null {
  if (typeof window === "undefined") return null;
  const params = new URLSearchParams(window.location.search);
  if (params.get("backend") === "http") {
    return params.get("api_url");
  }
  return null;
}

/**
 * Check if the active backend supports native plugin loading.
 *
 * Returns `true` for Tauri (always) and for the HTTP backend when the
 * server was built with the `plugins` feature.  The HTTP backend stores
 * the `native_plugins` flag from the `/api/workspace` init response.
 *
 * Also caches the result in a module-level variable so subsequent checks
 * (including non-reactive `$derived` evaluations) pick up the value
 * even if called from different module instances.
 */
let _nativePluginCached: boolean | null = null;

export function isNativePluginBackend(): boolean {
  if (isTauri()) return true;
  if (_nativePluginCached !== null) return _nativePluginCached;

  // Check the HTTP backend's flag via the globalThis singleton.
  const instance = (globalThis as any).__diaryx_backendInstance;
  if (instance && typeof instance.nativePlugins === "boolean") {
    _nativePluginCached = instance.nativePlugins as boolean;
    return instance.nativePlugins as boolean;
  }
  return false;
}

/**
 * Set the native plugin backend flag explicitly.
 *
 * Called by the HTTP backend after init so the flag is available
 * immediately, even before the globalThis singleton is readable
 * from other module instances.
 */
export function setNativePluginBackend(value: boolean): void {
  _nativePluginCached = value;
}

/**
 * Check if running in a browser (non-Tauri) environment.
 */
export function isBrowser(): boolean {
  return typeof window !== "undefined" && !isTauri();
}
