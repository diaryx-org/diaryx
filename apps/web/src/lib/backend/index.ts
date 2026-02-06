// Backend factory - auto-detects runtime environment and provides appropriate backend

import type { Backend } from "./interface";
import { isTauri, isBrowser } from "./interface";
import { createApi, type Api } from "./api";

// Re-export types and utilities
export type {
  Backend,
  Config,
  TreeNode,
  EntryData,
  SearchResults,
  SearchOptions,
  CreateEntryOptions,
  TemplateInfo,
  SearchMatch,
  FileSearchResult,
  ValidationResult,
  ValidationResultWithMeta,
  ValidationError,
  ValidationErrorWithMeta,
  ValidationWarning,
  ValidationWarningWithMeta,
  ExportPlan,
  ExportedFile,
  BinaryExportFile,
  StorageInfo,
} from "./interface";

export { BackendError, isTauri, isBrowser } from "./interface";

// Re-export API types from generated
export type { CreateChildResult } from "./generated";

// Re-export API wrapper
export { createApi, type Api } from "./api";

// ============================================================================
// Singleton Backend Instance
// ============================================================================

// Use globalThis to ensure the singleton is shared across module instances.
// Vite dev server may create separate module instances for the same file when
// imported via different paths (e.g., "$lib/backend" vs "/src/lib/backend").
// Without globalThis, each module instance gets its own backendInstance/initPromise,
// causing duplicate Worker creation and lost event subscriptions.
const _g = globalThis as any;

function getBackendInstance(): Backend | null {
  return _g.__diaryx_backendInstance ?? null;
}
function setBackendInstance(b: Backend | null): void {
  _g.__diaryx_backendInstance = b;
}
function getInitPromise(): Promise<Backend> | null {
  return _g.__diaryx_initPromise ?? null;
}
function setInitPromise(p: Promise<Backend> | null): void {
  _g.__diaryx_initPromise = p;
}

/**
 * Get the backend instance, creating it if necessary.
 * This is the main entry point for the backend abstraction.
 *
 * Usage:
 * ```ts
 * const backend = await getBackend();
 * const config = await backend.getConfig();
 * ```
 */
export async function getBackend(): Promise<Backend> {
  const existing = getBackendInstance();
  if (existing?.isReady()) {
    return existing;
  }

  // Prevent multiple simultaneous initializations
  const pending = getInitPromise();
  if (pending) {
    return pending;
  }

  console.log("[Backend] Starting initialization...");
  const promise = initializeBackend();
  setInitPromise(promise);
  return promise;
}

/**
 * Initialize the appropriate backend based on runtime environment.
 */
async function initializeBackend(): Promise<Backend> {
  console.log("[Backend] Detecting runtime environment...");
  console.log("[Backend] isTauri():", isTauri());
  console.log("[Backend] isBrowser():", isBrowser());
  console.log(
    "[Backend] window.__TAURI__:",
    typeof window !== "undefined" ? (window as any).__TAURI__ : "N/A",
  );

  try {
    let instance: Backend;
    if (isTauri()) {
      console.log("[Backend] Using Tauri backend");
      const { TauriBackend } = await import("./tauri");
      instance = new TauriBackend();
    } else if (isBrowser()) {
      // Use WorkerBackend which runs WasmBackend in a Web Worker
      // This enables OPFS with createSyncAccessHandle() for Safari
      console.log("[Backend] Using WorkerBackend (WASM in Web Worker)");
      const { WorkerBackendNew } = await import("./workerBackendNew");
      instance = new WorkerBackendNew();
    } else {
      throw new Error("Unsupported runtime environment");
    }

    console.log("[Backend] Calling backend.init()...");
    await instance.init();
    console.log("[Backend] Backend initialized successfully");
    setBackendInstance(instance);
    return instance;
  } catch (error) {
    console.error("[Backend] Initialization failed:", error);
    // Reset state so we can retry
    setBackendInstance(null);
    setInitPromise(null);
    throw error;
  }
}

/**
 * Reset the backend instance (useful for testing).
 */
export function resetBackend(): void {
  console.log("[Backend] Resetting backend instance");
  setBackendInstance(null);
  setInitPromise(null);
}

// ============================================================================
// Convenience Functions
// ============================================================================

/**
 * Check if the backend is ready to use.
 */
export function isBackendReady(): boolean {
  return getBackendInstance()?.isReady() ?? false;
}

/**
 * Get the backend instance synchronously.
 * Throws if the backend hasn't been initialized yet.
 */
export function getBackendSync(): Backend {
  const instance = getBackendInstance();
  if (!instance?.isReady()) {
    throw new Error(
      "Backend not initialized. Call getBackend() first and await it.",
    );
  }
  return instance;
}

// ============================================================================
// API Wrapper Access
// ============================================================================

let apiInstance: Api | null = null;

/**
 * Get the typed API wrapper, initializing if necessary.
 * This is the recommended way to interact with the backend.
 *
 * Usage:
 * ```ts
 * const api = await getApi();
 * const entry = await api.getEntry('workspace/notes.md');
 * ```
 */
export async function getApi(): Promise<Api> {
  if (apiInstance) {
    return apiInstance;
  }
  const backend = await getBackend();
  apiInstance = createApi(backend);
  return apiInstance;
}

/**
 * Get the API wrapper synchronously.
 * Throws if the backend hasn't been initialized yet.
 */
export function getApiSync(): Api {
  if (apiInstance) {
    return apiInstance;
  }
  const backend = getBackendSync();
  apiInstance = createApi(backend);
  return apiInstance;
}

// ============================================================================
// Auto-Persist Hook (Deprecated/Removed)
// ============================================================================

// startAutoPersist/persistNow removed - persistence is handled automatically by the backend
