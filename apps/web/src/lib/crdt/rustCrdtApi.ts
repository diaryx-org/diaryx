/**
 * Type-safe API wrapper for Rust CRDT commands.
 *
 * All CRDT commands are routed through PluginCommand { plugin: "sync", ... }.
 * Responses come back as PluginResult(json) where the json is the raw
 * response from the sync plugin's dispatch() method.
 */

import type { Backend } from '../backend/interface';
import { generateUUID } from '$lib/utils';
import type { FileMetadata } from '../backend/generated';
import type { CrdtHistoryEntry, FileDiff } from './types';

/**
 * Execute a sync plugin command via PluginCommand routing.
 * Returns the raw JSON data from the sync plugin.
 */
async function executeSyncCommand(
  backend: Backend,
  command: string,
  params: Record<string, unknown> = {}
): Promise<unknown> {
  const response = await backend.execute({
    type: 'PluginCommand',
    params: { plugin: 'sync', command, params },
  } as any);

  // Response is { type: 'PluginResult', data: <json from plugin> }
  if (response.type === 'PluginResult') {
    return response.data;
  }
  if (response.type === 'Ok') {
    return null;
  }
  throw new Error(`Expected PluginResult, got ${response.type}: ${JSON.stringify(response)}`);
}

/** Decode base64 string to Uint8Array. */
function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** Encode Uint8Array to base64 string. */
function encodeBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Extract base64-encoded binary data from a plugin response. */
function extractBinary(result: unknown, context: string): Uint8Array {
  if (result == null) return new Uint8Array(0);
  const obj = result as Record<string, unknown>;
  const b64 = obj.data;
  if (typeof b64 !== 'string') {
    throw new Error(`Expected base64 'data' field for ${context}, got: ${JSON.stringify(result)}`);
  }
  return decodeBase64(b64);
}

/**
 * CRDT API wrapper providing type-safe access to Rust CRDT operations.
 */
export class RustCrdtApi {
  constructor(private backend: Backend) {}

  // ===========================================================================
  // Workspace CRDT Operations
  // ===========================================================================

  async getSyncState(docName: string = 'workspace'): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetSyncState', { doc_name: docName });
    return extractBinary(result, 'GetSyncState');
  }

  async applyRemoteUpdate(
    update: Uint8Array,
    docName: string = 'workspace'
  ): Promise<bigint | null> {
    const result = await executeSyncCommand(this.backend, 'ApplyRemoteUpdate', {
      doc_name: docName,
      update: encodeBase64(update),
    });
    const obj = result as Record<string, unknown> | null;
    return obj?.update_id != null ? BigInt(obj.update_id as number) : null;
  }

  async getMissingUpdates(
    remoteStateVector: Uint8Array,
    docName: string = 'workspace'
  ): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetMissingUpdates', {
      doc_name: docName,
      remote_state_vector: encodeBase64(remoteStateVector),
    });
    return extractBinary(result, 'GetMissingUpdates');
  }

  async getFullState(docName: string = 'workspace'): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetFullState', { doc_name: docName });
    return extractBinary(result, 'GetFullState');
  }

  // ===========================================================================
  // History Operations
  // ===========================================================================

  async getHistory(docName: string = 'workspace', limit?: number): Promise<CrdtHistoryEntry[]> {
    console.log('[RustCrdtApi] getHistory:', docName, 'limit:', limit);
    const result = await executeSyncCommand(this.backend, 'GetHistory', {
      doc_name: docName,
      limit: limit ?? null,
    });
    const history = (result ?? []) as CrdtHistoryEntry[];
    console.log('[RustCrdtApi] getHistory result:', history.length, 'entries');
    return history;
  }

  async getFileHistory(filePath: string, limit?: number): Promise<CrdtHistoryEntry[]> {
    console.log('[RustCrdtApi] getFileHistory:', filePath, 'limit:', limit);
    const result = await executeSyncCommand(this.backend, 'GetFileHistory', {
      file_path: filePath,
      limit: limit ?? null,
    });
    const history = (result ?? []) as CrdtHistoryEntry[];
    console.log('[RustCrdtApi] getFileHistory result:', history.length, 'entries');
    return history;
  }

  async restoreVersion(updateId: bigint, docName: string = 'workspace'): Promise<void> {
    await executeSyncCommand(this.backend, 'RestoreVersion', {
      doc_name: docName,
      update_id: Number(updateId),
    });
  }

  async getVersionDiff(
    fromId: bigint,
    toId: bigint,
    docName: string = 'workspace'
  ): Promise<FileDiff[]> {
    const result = await executeSyncCommand(this.backend, 'GetVersionDiff', {
      doc_name: docName,
      from_id: Number(fromId),
      to_id: Number(toId),
    });
    return (result ?? []) as FileDiff[];
  }

  async getStateAt(updateId: bigint, docName: string = 'workspace'): Promise<Uint8Array | null> {
    const result = await executeSyncCommand(this.backend, 'GetStateAt', {
      doc_name: docName,
      update_id: Number(updateId),
    });
    if (result == null) return null;
    return extractBinary(result, 'GetStateAt');
  }

  // ===========================================================================
  // File Metadata Operations
  // ===========================================================================

  /**
   * Get file metadata from the CRDT by key (doc_id or path).
   * @deprecated Use getFileById() for doc-ID based access
   */
  async getFile(path: string): Promise<FileMetadata | null> {
    const result = await executeSyncCommand(this.backend, 'GetCrdtFile', { path });
    return (result ?? null) as FileMetadata | null;
  }

  async getFileById(docId: string): Promise<FileMetadata | null> {
    return this.getFile(docId);
  }

  /**
   * Set file metadata in the CRDT.
   * @deprecated Use setFileById() for doc-ID based access
   */
  async setFile(path: string, metadata: FileMetadata): Promise<void> {
    console.log('[RustCrdtApi] setFile:', path);
    await executeSyncCommand(this.backend, 'SetCrdtFile', {
      path,
      metadata: metadata as unknown,
    });
    console.log('[RustCrdtApi] setFile complete:', path);
  }

  async setFileById(docId: string, metadata: FileMetadata): Promise<void> {
    return this.setFile(docId, metadata);
  }

  async createFile(metadata: FileMetadata): Promise<string> {
    const docId = generateUUID();
    await this.setFile(docId, metadata);
    console.log('[RustCrdtApi] createFile: generated doc_id', docId);
    return docId;
  }

  async listFiles(includeDeleted: boolean = false): Promise<[string, FileMetadata][]> {
    const result = await executeSyncCommand(this.backend, 'ListCrdtFiles', {
      include_deleted: includeDeleted,
    });
    return (result ?? []) as [string, FileMetadata][];
  }

  async findDocIdByPath(path: string): Promise<string | null> {
    const files = await this.listFiles(false);
    const pathParts = path.split('/').filter(p => p.length > 0);

    if (pathParts.length === 0) return null;

    const filesByFilename = new Map<string, [string, FileMetadata][]>();
    for (const [docId, meta] of files) {
      const existing = filesByFilename.get(meta.filename) || [];
      existing.push([docId, meta]);
      filesByFilename.set(meta.filename, existing);
    }

    const targetFilename = pathParts[pathParts.length - 1];
    const candidates = filesByFilename.get(targetFilename) || [];

    for (const [docId] of candidates) {
      const derivedPath = await this.getPathForDocId(docId);
      if (derivedPath === path) {
        return docId;
      }
    }

    const legacyMatch = files.find(([key]) => key === path);
    if (legacyMatch) {
      return legacyMatch[0];
    }

    return null;
  }

  async getPathForDocId(docId: string): Promise<string | null> {
    const files = await this.listFiles(false);
    const fileMap = new Map(files);

    const parts: string[] = [];
    let current = docId;
    const visited = new Set<string>();

    while (current) {
      if (visited.has(current)) {
        console.warn('[RustCrdtApi] Circular reference in getPathForDocId:', docId);
        return null;
      }
      visited.add(current);

      const meta = fileMap.get(current);
      if (!meta) {
        if (current.includes('/')) {
          parts.unshift(...current.split('/'));
          break;
        }
        return null;
      }

      if (!meta.filename) {
        console.warn('[RustCrdtApi] Empty filename for doc_id:', current);
        return null;
      }

      parts.unshift(meta.filename);

      if (meta.part_of) {
        if (meta.part_of.includes('/') || meta.part_of.endsWith('.md')) {
          const parentDir = meta.part_of.split('/').slice(0, -1).join('/');
          if (parentDir) {
            parts.unshift(...parentDir.split('/'));
          }
          break;
        }
        current = meta.part_of;
      } else {
        break;
      }
    }

    return parts.join('/');
  }

  async saveCrdtState(docName: string = 'workspace'): Promise<void> {
    await executeSyncCommand(this.backend, 'SaveCrdtState', { doc_name: docName });
  }

  // ===========================================================================
  // Body Document Operations
  // ===========================================================================

  /**
   * @deprecated Use getBodyContentById() for doc-ID based access
   */
  async getBodyContent(docName: string): Promise<string> {
    const result = await executeSyncCommand(this.backend, 'GetBodyContent', { doc_name: docName });
    const obj = result as Record<string, unknown> | null;
    return (obj?.content as string) ?? '';
  }

  async getBodyContentById(docId: string): Promise<string> {
    return this.getBodyContent(docId);
  }

  /**
   * @deprecated Use setBodyContentById() for doc-ID based access
   */
  async setBodyContent(docName: string, content: string): Promise<void> {
    await executeSyncCommand(this.backend, 'SetBodyContent', { doc_name: docName, content });
  }

  async setBodyContentById(docId: string, content: string): Promise<void> {
    return this.setBodyContent(docId, content);
  }

  async resetBodyDoc(docName: string): Promise<void> {
    await executeSyncCommand(this.backend, 'ResetBodyDoc', { doc_name: docName });
  }

  async getBodySyncState(docName: string): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetBodySyncState', { doc_name: docName });
    return extractBinary(result, 'GetBodySyncState');
  }

  async getBodyFullState(docName: string): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetBodyFullState', { doc_name: docName });
    return extractBinary(result, 'GetBodyFullState');
  }

  async applyBodyUpdate(docName: string, update: Uint8Array): Promise<bigint | null> {
    const result = await executeSyncCommand(this.backend, 'ApplyBodyUpdate', {
      doc_name: docName,
      update: encodeBase64(update),
    });
    const obj = result as Record<string, unknown> | null;
    return obj?.update_id != null ? BigInt(obj.update_id as number) : null;
  }

  async getBodyMissingUpdates(
    docName: string,
    remoteStateVector: Uint8Array
  ): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'GetBodyMissingUpdates', {
      doc_name: docName,
      remote_state_vector: encodeBase64(remoteStateVector),
    });
    return extractBinary(result, 'GetBodyMissingUpdates');
  }

  async saveBodyDoc(docName: string): Promise<void> {
    await executeSyncCommand(this.backend, 'SaveBodyDoc', { doc_name: docName });
  }

  async saveAllBodyDocs(): Promise<void> {
    await executeSyncCommand(this.backend, 'SaveAllBodyDocs');
  }

  async listLoadedBodyDocs(): Promise<string[]> {
    const result = await executeSyncCommand(this.backend, 'ListLoadedBodyDocs');
    return (result ?? []) as string[];
  }

  async unloadBodyDoc(docName: string): Promise<void> {
    await executeSyncCommand(this.backend, 'UnloadBodyDoc', { doc_name: docName });
  }

  // ===========================================================================
  // Sync Protocol Operations
  // ===========================================================================

  async createSyncStep1(docName: string = 'workspace'): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'CreateSyncStep1', { doc_name: docName });
    return extractBinary(result, 'CreateSyncStep1');
  }

  async handleSyncMessage(
    message: Uint8Array,
    docName: string = 'workspace',
    writeToDisk: boolean = false
  ): Promise<Uint8Array | null> {
    const result = await executeSyncCommand(this.backend, 'HandleSyncMessage', {
      doc_name: docName,
      message: encodeBase64(message),
      write_to_disk: writeToDisk,
    });
    if (result == null) return null;
    const bytes = extractBinary(result, 'HandleSyncMessage');
    return bytes.length > 0 ? bytes : null;
  }

  async createUpdateMessage(update: Uint8Array, docName: string = 'workspace'): Promise<Uint8Array> {
    const result = await executeSyncCommand(this.backend, 'CreateUpdateMessage', {
      doc_name: docName,
      update: encodeBase64(update),
    });
    return extractBinary(result, 'CreateUpdateMessage');
  }
}

/**
 * Create a CRDT API wrapper for a backend instance.
 */
export function createCrdtApi(backend: Backend): RustCrdtApi {
  return new RustCrdtApi(backend);
}
