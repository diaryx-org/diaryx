/**
 * File Hashing Utilities for P2P Sync
 *
 * Provides content hashing and file manifest generation for
 * comparing files between peers during P2P sync.
 */

import type { Api } from '../backend/api';
import type { TreeNode } from '../backend';

// ============================================================================
// Types
// ============================================================================

export interface FileInfo {
  /** File path relative to workspace root */
  path: string;
  /** SHA-256 hash of file content */
  hash: string;
  /** File size in bytes */
  size: number;
  /** Last modified timestamp (Unix ms) */
  modified: number;
}

export interface FileManifest {
  /** List of all files in the workspace */
  files: FileInfo[];
  /** Timestamp when manifest was generated */
  generatedAt: number;
}

// ============================================================================
// Hashing
// ============================================================================

/**
 * Compute SHA-256 hash of content.
 */
export async function hashContent(content: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(content);
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

/**
 * Compute a quick hash for comparison (first 16 chars of SHA-256).
 */
export async function quickHash(content: string): Promise<string> {
  const full = await hashContent(content);
  return full.slice(0, 16);
}

// ============================================================================
// Manifest Generation
// ============================================================================

/**
 * Generate a manifest of all files in the workspace.
 * Traverses the file tree and computes hashes for each file.
 */
export async function generateFileManifest(api: Api): Promise<FileManifest> {
  const files: FileInfo[] = [];

  // Get the workspace tree
  const tree = await api.getWorkspaceTree();
  if (!tree) {
    return { files: [], generatedAt: Date.now() };
  }

  // Recursively collect all files
  await collectFiles(api, tree, files);

  return {
    files,
    generatedAt: Date.now(),
  };
}

/**
 * Recursively collect file info from tree nodes.
 */
async function collectFiles(
  api: Api,
  node: TreeNode,
  files: FileInfo[]
): Promise<void> {
  // Skip if no path
  if (!node.path) return;

  try {
    // Get file content and metadata
    const entry = await api.getEntry(node.path);
    if (entry) {
      const content = entry.content || '';
      const hash = await quickHash(content);

      files.push({
        path: node.path,
        hash,
        size: new TextEncoder().encode(content).length,
        modified: Date.now(), // TODO: Get actual modified time from metadata
      });
    }
  } catch (error) {
    console.warn(`[FileHash] Failed to hash file: ${node.path}`, error);
  }

  // Process children
  if (node.children) {
    for (const child of node.children) {
      await collectFiles(api, child, files);
    }
  }
}

// ============================================================================
// Comparison
// ============================================================================

export interface FileComparison {
  /** Files that exist on remote but not locally */
  missing: FileInfo[];
  /** Files that exist locally but not on remote */
  extra: FileInfo[];
  /** Files that exist on both but have different content */
  modified: Array<{
    path: string;
    local: FileInfo;
    remote: FileInfo;
  }>;
  /** Files that are identical on both sides */
  matching: string[];
}

/**
 * Compare local manifest with remote manifest.
 */
export function compareManifests(
  local: FileManifest,
  remote: FileManifest
): FileComparison {
  const localMap = new Map(local.files.map((f) => [f.path, f]));
  const remoteMap = new Map(remote.files.map((f) => [f.path, f]));

  const result: FileComparison = {
    missing: [],
    extra: [],
    modified: [],
    matching: [],
  };

  // Find missing and modified files
  for (const [path, remoteFile] of remoteMap) {
    const localFile = localMap.get(path);

    if (!localFile) {
      // File exists on remote but not locally
      result.missing.push(remoteFile);
    } else if (localFile.hash !== remoteFile.hash) {
      // File exists on both but content differs
      result.modified.push({
        path,
        local: localFile,
        remote: remoteFile,
      });
    } else {
      // Files match
      result.matching.push(path);
    }
  }

  // Find extra files (exist locally but not on remote)
  for (const [path, localFile] of localMap) {
    if (!remoteMap.has(path)) {
      result.extra.push(localFile);
    }
  }

  return result;
}

/**
 * Determine which files need to be transferred based on comparison.
 */
export function getFilesToSync(
  comparison: FileComparison,
  conflictThresholdMs: number = 5 * 60 * 1000 // 5 minutes
): {
  download: string[]; // Files to download from remote
  upload: string[]; // Files to upload to remote
  conflicts: string[]; // Files that need manual resolution
} {
  const download: string[] = [];
  const upload: string[] = [];
  const conflicts: string[] = [];

  // Missing files should be downloaded
  download.push(...comparison.missing.map((f) => f.path));

  // Extra files should be uploaded
  upload.push(...comparison.extra.map((f) => f.path));

  // Modified files need conflict resolution
  for (const mod of comparison.modified) {
    const timeDiff = Math.abs(mod.local.modified - mod.remote.modified);

    if (timeDiff < conflictThresholdMs) {
      // Modified around the same time - needs manual resolution
      conflicts.push(mod.path);
    } else if (mod.remote.modified > mod.local.modified) {
      // Remote is newer - download
      download.push(mod.path);
    } else {
      // Local is newer - upload
      upload.push(mod.path);
    }
  }

  return { download, upload, conflicts };
}
