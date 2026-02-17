/**
 * Storage module for Diaryx web app.
 *
 * This module provides persistent CRDT storage using SQLite (via sql.js)
 * with OPFS for durability.
 */

export {
  SqliteStorage,
  getSqliteStorage,
  getSqliteStorageSync,
  flushSqliteStorage,
  resetSqliteStorage,
  type CrdtUpdate,
  type DbPersistence,
  DirectoryHandlePersistence,
} from "./sqliteStorage.js";

export {
  initializeSqliteStorage,
  isStorageReady,
  flushStorage,
} from "./sqliteStorageBridge.js";

export {
  getLocalWorkspaces,
  getLocalWorkspace,
  getCurrentWorkspaceId,
  isWorkspaceLocal,
  addLocalWorkspace,
  removeLocalWorkspace,
  setCurrentWorkspaceId,
  clearCurrentWorkspaceId,
  renameLocalWorkspace,
  type LocalWorkspace,
} from "./localWorkspaceRegistry.svelte.js";

/**
 * Set up the global bridge that Rust WASM code will use to access storage.
 *
 * Call this BEFORE creating DiaryxBackend to enable persistent CRDT storage.
 * Works in both Window and Worker contexts using globalThis.
 *
 * @param persistence - Where to persist the database. If omitted, no persistence.
 *
 * @example
 * ```typescript
 * import { setupCrdtStorageBridge, DirectoryHandlePersistence } from './lib/storage';
 * const handle = await navigator.storage.getDirectory();
 * const wsDir = await handle.getDirectoryHandle('My Journal', { create: true });
 * await setupCrdtStorageBridge(new DirectoryHandlePersistence(wsDir));
 * const backend = await DiaryxBackend.createOpfs('My Journal');
 * ```
 */
export async function setupCrdtStorageBridge(persistence?: import("./sqliteStorage.js").DbPersistence | null): Promise<void> {
  // Dynamically import to avoid circular dependencies
  const bridge = await import("./sqliteStorageBridge.js");

  // Initialize the storage with the provided persistence adapter
  await bridge.initializeSqliteStorage(persistence);

  // Set up the global bridge object that Rust will access
  // Use globalThis for compatibility with both Window and Worker contexts
  (globalThis as any).__diaryx_crdt_storage = {
    crdt_load_doc: bridge.crdt_load_doc,
    crdt_save_doc: bridge.crdt_save_doc,
    crdt_delete_doc: bridge.crdt_delete_doc,
    crdt_list_docs: bridge.crdt_list_docs,
    crdt_append_update: bridge.crdt_append_update,
    crdt_get_updates_since: bridge.crdt_get_updates_since,
    crdt_get_all_updates: bridge.crdt_get_all_updates,
    crdt_get_latest_update_id: bridge.crdt_get_latest_update_id,
    crdt_compact: bridge.crdt_compact,
    crdt_clear_updates: bridge.crdt_clear_updates,
    crdt_rename_doc: bridge.crdt_rename_doc,
    crdt_update_file_index: bridge.crdt_update_file_index,
    crdt_query_active_files: bridge.crdt_query_active_files,
    crdt_remove_from_file_index: bridge.crdt_remove_from_file_index,
  };

  console.log("[Storage] CRDT storage bridge initialized");
}
