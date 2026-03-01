/**
 * CRDT-specific command, response, and data types.
 *
 * These types are standalone — they don't extend the generated Command/Response
 * types. CRDT commands are routed as PluginCommand { plugin: "sync", ... }
 * and responses come back as PluginResult(json).
 */

import type { FileMetadata } from '../backend/generated';

// ============================================================================
// CRDT Data Types (previously generated from diaryx_core, now hand-maintained)
// ============================================================================

/** CRDT history entry for version tracking. */
export type CrdtHistoryEntry = {
  update_id: bigint;
  timestamp: bigint;
  origin: string;
  files_changed: string[];
  device_id: string | null;
  device_name: string | null;
};

/** Type of change in a file diff. */
export type ChangeType = "Added" | "Modified" | "Deleted" | "Renamed" | "Restored";

/** Diff between two file versions. */
export type FileDiff = {
  path: string;
  change_type: ChangeType;
  old_value: string | null;
  new_value: string | null;
};

// ============================================================================
// CRDT Commands (sent as PluginCommand { plugin: "sync", command, params })
// ============================================================================

/** All CRDT command types that can be sent to the sync plugin. */
export type CrdtCommand =
  // Workspace CRDT operations
  | { type: 'GetSyncState'; params: { doc_name: string } }
  | { type: 'ApplyRemoteUpdate'; params: { doc_name: string; update: number[] } }
  | { type: 'GetMissingUpdates'; params: { doc_name: string; remote_state_vector: number[] } }
  | { type: 'GetFullState'; params: { doc_name: string } }
  // History operations
  | { type: 'GetHistory'; params: { doc_name: string; limit: number | null } }
  | { type: 'GetFileHistory'; params: { file_path: string; limit: number | null } }
  | { type: 'RestoreVersion'; params: { doc_name: string; update_id: bigint } }
  | { type: 'GetVersionDiff'; params: { doc_name: string; from_id: bigint; to_id: bigint } }
  | { type: 'GetStateAt'; params: { doc_name: string; update_id: bigint } }
  // File metadata operations
  | { type: 'GetCrdtFile'; params: { path: string } }
  | { type: 'SetCrdtFile'; params: { path: string; metadata: unknown } }
  | { type: 'ListCrdtFiles'; params: { include_deleted: boolean } }
  | { type: 'SaveCrdtState'; params: { doc_name: string } }
  // Body document operations
  | { type: 'GetBodyContent'; params: { doc_name: string } }
  | { type: 'SetBodyContent'; params: { doc_name: string; content: string } }
  | { type: 'GetBodySyncState'; params: { doc_name: string } }
  | { type: 'GetBodyFullState'; params: { doc_name: string } }
  | { type: 'ApplyBodyUpdate'; params: { doc_name: string; update: number[] } }
  | { type: 'GetBodyMissingUpdates'; params: { doc_name: string; remote_state_vector: number[] } }
  | { type: 'SaveBodyDoc'; params: { doc_name: string } }
  | { type: 'SaveAllBodyDocs' }
  | { type: 'ListLoadedBodyDocs' }
  | { type: 'UnloadBodyDoc'; params: { doc_name: string } }
  // Sync protocol operations
  | { type: 'CreateSyncStep1'; params: { doc_name: string } }
  | { type: 'HandleSyncMessage'; params: { doc_name: string; message: number[]; write_to_disk: boolean } }
  | { type: 'CreateUpdateMessage'; params: { doc_name: string; update: number[] } };

// ============================================================================
// CRDT Responses (from PluginResult JSON)
// ============================================================================

/**
 * All possible response shapes from the sync plugin.
 *
 * Note: These come back wrapped in Response::PluginResult(json).
 * The rustCrdtApi layer unwraps the PluginResult and parses these.
 */
export type CrdtResponse =
  | { type: 'data'; data: string }        // base64-encoded binary
  | { type: 'update_id'; data: bigint | null }
  | { type: 'history'; data: CrdtHistoryEntry[] }
  | { type: 'version_diff'; data: FileDiff[] }
  | { type: 'file'; data: FileMetadata | null }
  | { type: 'files'; data: [string, FileMetadata][] }
  | { type: 'string'; data: string }
  | { type: 'strings'; data: string[] }
  | { type: 'ok' };
