/**
 * P2P File Transfer Protocol
 *
 * Handles bulk file synchronization between peers via WebRTC data channels.
 * Works alongside the existing y-webrtc CRDT sync.
 */

import type { Api } from '../backend/api';
import {
  generateFileManifest,
  compareManifests,
  getFilesToSync,
  hashContent,
  type FileManifest,
} from './fileHash';

// ============================================================================
// Types
// ============================================================================

/** Maximum chunk size for file transfers (16KB) */
const CHUNK_SIZE = 16 * 1024;

/** Timeout for receiving all chunks of a file (30 seconds) */
const CHUNK_TIMEOUT_MS = 30 * 1000;

/** Message types for P2P file transfer protocol */
export type P2PFileMessage =
  | { type: 'FILE_MANIFEST_REQUEST' }
  | { type: 'FILE_MANIFEST'; manifest: FileManifest }
  | { type: 'FILE_REQUEST'; paths: string[] }
  | { type: 'FILE_CONTENT'; path: string; content: string; hash: string; chunkIndex?: number; totalChunks?: number }
  | { type: 'FILE_ACK'; path: string; success: boolean; error?: string }
  | { type: 'SYNC_COMPLETE'; stats: SyncStats };

export interface SyncStats {
  downloaded: number;
  uploaded: number;
  conflicts: number;
  errors: number;
  duration: number;
}

export interface SyncProgress {
  phase: 'idle' | 'comparing' | 'downloading' | 'uploading' | 'complete' | 'error';
  currentFile?: string;
  filesTotal: number;
  filesComplete: number;
  bytesTotal: number;
  bytesComplete: number;
}

export interface SyncResult {
  success: boolean;
  downloaded: string[];
  uploaded: string[];
  conflicts: string[];
  errors: Array<{ path: string; error: string }>;
  stats: SyncStats;
}

export interface ConflictInfo {
  path: string;
  localModified: number;
  remoteModified: number;
  localHash: string;
  remoteHash: string;
}

type ProgressCallback = (progress: SyncProgress) => void;
type ConflictCallback = (conflicts: ConflictInfo[]) => Promise<Map<string, 'local' | 'remote' | 'both'>>;
type MessageHandler = (msg: P2PFileMessage) => void;

// ============================================================================
// P2P File Transfer Class
// ============================================================================

export class P2PFileTransfer {
  private api: Api;
  private messageHandlers: Set<MessageHandler> = new Set();
  private broadcastFn: ((msg: P2PFileMessage) => void) | null = null;
  
  // Sync state
  private isSyncing = false;
  private progress: SyncProgress = {
    phase: 'idle',
    filesTotal: 0,
    filesComplete: 0,
    bytesTotal: 0,
    bytesComplete: 0,
  };
  private progressCallbacks: Set<ProgressCallback> = new Set();
  private conflictCallback: ConflictCallback | null = null;
  
  // Pending file chunks (for reassembly)
  private pendingChunks: Map<string, { chunks: string[]; totalChunks: number }> = new Map();

  // Timeouts for pending chunk reassembly (to prevent indefinite waits)
  private pendingChunkTimeouts: Map<string, ReturnType<typeof setTimeout>> = new Map();

  // Pending requests
  private pendingManifest: ((manifest: FileManifest) => void) | null = null;
  private pendingAcks: Map<string, (success: boolean) => void> = new Map();

  constructor(api: Api) {
    this.api = api;
  }

  // ==========================================================================
  // Setup
  // ==========================================================================

  /**
   * Set the broadcast function for sending messages to peers.
   */
  setBroadcast(fn: (msg: P2PFileMessage) => void): void {
    this.broadcastFn = fn;
  }

  /**
   * Handle an incoming message from a peer.
   */
  async handleMessage(msg: P2PFileMessage): Promise<void> {
    // Notify registered handlers
    for (const handler of this.messageHandlers) {
      handler(msg);
    }

    // Process message
    switch (msg.type) {
      case 'FILE_MANIFEST_REQUEST':
        await this.handleManifestRequest();
        break;
      case 'FILE_MANIFEST':
        this.handleManifestResponse(msg.manifest);
        break;
      case 'FILE_REQUEST':
        await this.handleFileRequest(msg.paths);
        break;
      case 'FILE_CONTENT':
        await this.handleFileContent(msg);
        break;
      case 'FILE_ACK':
        this.handleFileAck(msg.path, msg.success);
        break;
      case 'SYNC_COMPLETE':
        console.log('[P2PFileTransfer] Peer sync complete:', msg.stats);
        break;
    }
  }

  /**
   * Register a message handler.
   */
  onMessage(handler: MessageHandler): () => void {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  /**
   * Register a progress callback.
   */
  onProgress(callback: ProgressCallback): () => void {
    this.progressCallbacks.add(callback);
    callback(this.progress); // Send current state immediately
    return () => this.progressCallbacks.delete(callback);
  }

  /**
   * Set the conflict resolution callback.
   */
  setConflictHandler(callback: ConflictCallback): void {
    this.conflictCallback = callback;
  }

  // ==========================================================================
  // Sync Initiation
  // ==========================================================================

  /**
   * Initiate a full sync with connected peers.
   */
  async initiateSync(): Promise<SyncResult> {
    if (this.isSyncing) {
      throw new Error('Sync already in progress');
    }

    const startTime = Date.now();
    this.isSyncing = true;

    const result: SyncResult = {
      success: true,
      downloaded: [],
      uploaded: [],
      conflicts: [],
      errors: [],
      stats: {
        downloaded: 0,
        uploaded: 0,
        conflicts: 0,
        errors: 0,
        duration: 0,
      },
    };

    try {
      // Phase 1: Compare manifests
      this.updateProgress({ phase: 'comparing', filesTotal: 0, filesComplete: 0, bytesTotal: 0, bytesComplete: 0 });

      const localManifest = await generateFileManifest(this.api);
      const remoteManifest = await this.requestManifest();

      if (!remoteManifest) {
        throw new Error('Failed to get remote manifest');
      }

      const comparison = compareManifests(localManifest, remoteManifest);
      const { download, upload, conflicts } = getFilesToSync(comparison);

      // Phase 2: Handle conflicts
      if (conflicts.length > 0 && this.conflictCallback) {
        const conflictInfos: ConflictInfo[] = conflicts.map((path) => {
          const mod = comparison.modified.find((m) => m.path === path)!;
          return {
            path,
            localModified: mod.local.modified,
            remoteModified: mod.remote.modified,
            localHash: mod.local.hash,
            remoteHash: mod.remote.hash,
          };
        });

        const resolutions = await this.conflictCallback(conflictInfos);

        for (const [path, resolution] of resolutions) {
          if (resolution === 'remote') {
            download.push(path);
          } else if (resolution === 'local') {
            upload.push(path);
          } else if (resolution === 'both') {
            // Keep both - download remote as .conflict file
            download.push(path);
            result.conflicts.push(path);
          }
        }
      } else {
        result.conflicts = conflicts;
      }

      // Phase 3: Download missing files
      if (download.length > 0) {
        this.updateProgress({
          phase: 'downloading',
          filesTotal: download.length,
          filesComplete: 0,
          bytesTotal: 0,
          bytesComplete: 0,
        });

        await this.downloadFiles(download, result);
      }

      // Phase 4: Upload extra files
      if (upload.length > 0) {
        this.updateProgress({
          phase: 'uploading',
          filesTotal: upload.length,
          filesComplete: 0,
          bytesTotal: 0,
          bytesComplete: 0,
        });

        await this.uploadFiles(upload, result);
      }

      // Complete
      result.stats = {
        downloaded: result.downloaded.length,
        uploaded: result.uploaded.length,
        conflicts: result.conflicts.length,
        errors: result.errors.length,
        duration: Date.now() - startTime,
      };

      this.updateProgress({ phase: 'complete', filesTotal: 0, filesComplete: 0, bytesTotal: 0, bytesComplete: 0 });
      this.broadcast({ type: 'SYNC_COMPLETE', stats: result.stats });

    } catch (error) {
      result.success = false;
      result.errors.push({
        path: '',
        error: error instanceof Error ? error.message : 'Unknown error',
      });
      this.updateProgress({ phase: 'error', filesTotal: 0, filesComplete: 0, bytesTotal: 0, bytesComplete: 0 });
    } finally {
      this.isSyncing = false;
    }

    return result;
  }

  /**
   * Get current sync progress.
   */
  getProgress(): SyncProgress {
    return { ...this.progress };
  }

  /**
   * Check if sync is in progress.
   */
  isSyncInProgress(): boolean {
    return this.isSyncing;
  }

  // ==========================================================================
  // Message Handlers
  // ==========================================================================

  private async handleManifestRequest(): Promise<void> {
    const manifest = await generateFileManifest(this.api);
    this.broadcast({ type: 'FILE_MANIFEST', manifest });
  }

  private handleManifestResponse(manifest: FileManifest): void {
    if (this.pendingManifest) {
      this.pendingManifest(manifest);
      this.pendingManifest = null;
    }
  }

  private async handleFileRequest(paths: string[]): Promise<void> {
    for (const path of paths) {
      try {
        const entry = await this.api.getEntry(path);
        if (!entry) {
          this.broadcast({
            type: 'FILE_ACK',
            path,
            success: false,
            error: 'File not found',
          });
          continue;
        }

        const content = entry.content || '';
        const hash = await hashContent(content);

        // Check if chunking is needed
        if (content.length > CHUNK_SIZE) {
          await this.sendChunkedFile(path, content, hash);
        } else {
          this.broadcast({
            type: 'FILE_CONTENT',
            path,
            content,
            hash,
          });
        }
      } catch (error) {
        this.broadcast({
          type: 'FILE_ACK',
          path,
          success: false,
          error: error instanceof Error ? error.message : 'Unknown error',
        });
      }
    }
  }

  private async handleFileContent(msg: P2PFileMessage & { type: 'FILE_CONTENT' }): Promise<void> {
    const { path, content, hash, chunkIndex, totalChunks } = msg;

    let fullContent = content;

    // Handle chunked content
    if (chunkIndex !== undefined && totalChunks !== undefined) {
      // Validate chunk bounds to prevent out-of-bounds access
      if (chunkIndex < 0 || chunkIndex >= totalChunks || totalChunks <= 0) {
        console.error(`[P2PFileTransfer] Invalid chunk index for ${path}: ${chunkIndex}/${totalChunks}`);
        this.broadcast({ type: 'FILE_ACK', path, success: false, error: 'Invalid chunk index' });
        return;
      }

      let pending = this.pendingChunks.get(path);
      if (!pending) {
        pending = { chunks: new Array(totalChunks).fill(''), totalChunks };
        this.pendingChunks.set(path, pending);

        // Set up timeout for chunk reassembly
        const existingTimeout = this.pendingChunkTimeouts.get(path);
        if (existingTimeout) {
          clearTimeout(existingTimeout);
        }
        const timeout = setTimeout(() => {
          const stillPending = this.pendingChunks.get(path);
          if (stillPending) {
            const received = stillPending.chunks.filter((c) => c !== '').length;
            console.error(
              `[P2PFileTransfer] Timeout waiting for chunks for ${path}: received ${received}/${stillPending.totalChunks}`
            );
            this.pendingChunks.delete(path);
            this.pendingChunkTimeouts.delete(path);
            this.broadcast({
              type: 'FILE_ACK',
              path,
              success: false,
              error: `Timeout: received ${received}/${stillPending.totalChunks} chunks`,
            });
          }
        }, CHUNK_TIMEOUT_MS);
        this.pendingChunkTimeouts.set(path, timeout);
      }

      // Validate totalChunks consistency
      if (pending.totalChunks !== totalChunks) {
        console.error(`[P2PFileTransfer] Inconsistent totalChunks for ${path}: expected ${pending.totalChunks}, got ${totalChunks}`);
        this.broadcast({ type: 'FILE_ACK', path, success: false, error: 'Inconsistent chunk metadata' });
        return;
      }

      pending.chunks[chunkIndex] = content;

      // Check if all chunks received
      const received = pending.chunks.filter((c) => c !== '').length;
      if (received < totalChunks) {
        return; // Wait for more chunks
      }

      // Clear the timeout since we have all chunks
      const timeout = this.pendingChunkTimeouts.get(path);
      if (timeout) {
        clearTimeout(timeout);
        this.pendingChunkTimeouts.delete(path);
      }

      fullContent = pending.chunks.join('');
      this.pendingChunks.delete(path);
    }

    // Verify hash
    const computedHash = await hashContent(fullContent);
    if (computedHash !== hash) {
      console.error(`[P2PFileTransfer] Hash mismatch for ${path}`);
      this.broadcast({ type: 'FILE_ACK', path, success: false, error: 'Hash mismatch' });
      return;
    }

    // Save the file
    try {
      await this.api.saveEntry(path, fullContent);
      this.broadcast({ type: 'FILE_ACK', path, success: true });

      // Update progress
      this.progress.filesComplete++;
      this.progress.bytesComplete += fullContent.length;
      this.notifyProgress();
    } catch (error) {
      console.error(`[P2PFileTransfer] Failed to save ${path}:`, error);
      this.broadcast({
        type: 'FILE_ACK',
        path,
        success: false,
        error: error instanceof Error ? error.message : 'Save failed',
      });
    }
  }

  private handleFileAck(path: string, success: boolean): void {
    const resolve = this.pendingAcks.get(path);
    if (resolve) {
      resolve(success);
      this.pendingAcks.delete(path);
    }
  }

  // ==========================================================================
  // Helpers
  // ==========================================================================

  private async requestManifest(): Promise<FileManifest | null> {
    return new Promise((resolve) => {
      this.pendingManifest = resolve;
      this.broadcast({ type: 'FILE_MANIFEST_REQUEST' });

      // Timeout after 10 seconds
      setTimeout(() => {
        if (this.pendingManifest) {
          this.pendingManifest = null;
          resolve(null);
        }
      }, 10000);
    });
  }

  private async downloadFiles(paths: string[], result: SyncResult): Promise<void> {
    // Request files in batches to avoid overwhelming the connection
    const BATCH_SIZE = 5;

    for (let i = 0; i < paths.length; i += BATCH_SIZE) {
      const batch = paths.slice(i, i + BATCH_SIZE);
      this.broadcast({ type: 'FILE_REQUEST', paths: batch });

      // Wait for acknowledgments
      await Promise.all(
        batch.map((path) =>
          new Promise<void>((resolve) => {
            this.pendingAcks.set(path, (success) => {
              if (success) {
                result.downloaded.push(path);
              } else {
                result.errors.push({ path, error: 'Download failed' });
              }
              resolve();
            });

            // Timeout after 30 seconds per file
            setTimeout(() => {
              if (this.pendingAcks.has(path)) {
                this.pendingAcks.delete(path);
                result.errors.push({ path, error: 'Timeout' });
                resolve();
              }
            }, 30000);
          })
        )
      );
    }
  }

  private async uploadFiles(paths: string[], result: SyncResult): Promise<void> {
    for (const path of paths) {
      try {
        const entry = await this.api.getEntry(path);
        if (!entry) {
          result.errors.push({ path, error: 'File not found' });
          continue;
        }

        const content = entry.content || '';
        const hash = await hashContent(content);

        if (content.length > CHUNK_SIZE) {
          await this.sendChunkedFile(path, content, hash);
        } else {
          this.broadcast({ type: 'FILE_CONTENT', path, content, hash });
        }

        result.uploaded.push(path);
        this.progress.filesComplete++;
        this.progress.bytesComplete += content.length;
        this.notifyProgress();
      } catch (error) {
        result.errors.push({
          path,
          error: error instanceof Error ? error.message : 'Unknown error',
        });
      }
    }
  }

  private async sendChunkedFile(path: string, content: string, hash: string): Promise<void> {
    const totalChunks = Math.ceil(content.length / CHUNK_SIZE);

    for (let i = 0; i < totalChunks; i++) {
      const chunk = content.slice(i * CHUNK_SIZE, (i + 1) * CHUNK_SIZE);
      this.broadcast({
        type: 'FILE_CONTENT',
        path,
        content: chunk,
        hash,
        chunkIndex: i,
        totalChunks,
      });

      // Small delay between chunks to prevent overwhelming the connection
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  private broadcast(msg: P2PFileMessage): void {
    if (this.broadcastFn) {
      this.broadcastFn(msg);
    } else {
      console.warn('[P2PFileTransfer] No broadcast function set');
    }
  }

  private updateProgress(update: Partial<SyncProgress>): void {
    this.progress = { ...this.progress, ...update };
    this.notifyProgress();
  }

  private notifyProgress(): void {
    for (const callback of this.progressCallbacks) {
      callback(this.progress);
    }
  }

  /**
   * Clean up resources including pending chunk timeouts.
   */
  destroy(): void {
    // Clear all pending chunk timeouts
    for (const timeout of this.pendingChunkTimeouts.values()) {
      clearTimeout(timeout);
    }
    this.pendingChunkTimeouts.clear();
    this.pendingChunks.clear();
    this.pendingAcks.clear();
    this.messageHandlers.clear();
    this.progressCallbacks.clear();
    this.broadcastFn = null;
  }
}

// ============================================================================
// Factory
// ============================================================================

let fileTransferInstance: P2PFileTransfer | null = null;

/**
 * Get or create the P2P file transfer instance.
 */
export function getFileTransfer(api: Api): P2PFileTransfer {
  if (!fileTransferInstance) {
    fileTransferInstance = new P2PFileTransfer(api);
  }
  return fileTransferInstance;
}

/**
 * Reset the file transfer instance (for testing).
 */
export function resetFileTransfer(): void {
  if (fileTransferInstance) {
    fileTransferInstance.destroy();
    fileTransferInstance = null;
  }
}
