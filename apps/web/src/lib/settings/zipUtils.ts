/**
 * Zip export/import utilities.
 *
 * Extracted from BackupSettings.svelte and WorkerBackendNew so the logic can
 * be unit-tested without a real Worker or Svelte component.
 */

import type JSZipType from 'jszip';

// ============================================================================
// Types
// ============================================================================

/** Minimal tree node shape returned by getFilesystemTree(). */
export interface TreeNode {
  path: string;
  children?: TreeNode[];
}

/** Callbacks the export walker uses to read files. */
export interface ExportFileReader {
  readText(path: string): Promise<string>;
  readBinary(path: string): Promise<Uint8Array>;
}

/** Callbacks the importer uses to write files. */
export interface ImportFileWriter {
  writeText(path: string, content: string): Promise<void>;
  writeBinary(path: string, data: Uint8Array): Promise<void>;
}

// ============================================================================
// Export — tree walker
// ============================================================================

const TEXT_EXTS = new Set(['md', 'txt', 'json', 'yaml', 'yml', 'toml']);

/**
 * Recursively walk a filesystem tree and add every non-hidden file to `zip`.
 *
 * Key behaviour: a node whose `path` looks like a file (has a `.` extension)
 * **and** has children is an index file — we add the file itself first, then
 * recurse into the children. This ensures index files are never skipped.
 *
 * @returns The number of files added.
 */
export async function addFilesToZip(
  zip: JSZipType,
  node: TreeNode,
  basePath: string,
  reader: ExportFileReader,
): Promise<number> {
  let count = 0;

  // Skip hidden files/directories
  const name = node.path.split('/').pop() || '';
  if (name.startsWith('.')) {
    return 0;
  }

  // Add this node's own file (index files have children but are also files)
  const hasFileExtension = name.includes('.');
  if (hasFileExtension) {
    const ok = await addSingleFileToZip(zip, node.path, basePath, reader);
    if (ok) count++;
  }

  // Recurse into children
  if (node.children && node.children.length > 0) {
    for (const child of node.children) {
      count += await addFilesToZip(zip, child, basePath, reader);
    }
  }

  return count;
}

async function addSingleFileToZip(
  zip: JSZipType,
  filePath: string,
  basePath: string,
  reader: ExportFileReader,
): Promise<boolean> {
  const relativePath = filePath.startsWith(basePath + '/')
    ? filePath.substring(basePath.length + 1)
    : filePath;

  try {
    const ext = filePath.split('.').pop()?.toLowerCase() || '';

    if (TEXT_EXTS.has(ext)) {
      const content = await reader.readText(filePath);
      zip.file(relativePath, content);
    } else {
      const data = await reader.readBinary(filePath);
      zip.file(relativePath, data, { binary: true });
    }
    return true;
  } catch (e) {
    console.warn(`[Export] Failed to read ${filePath}:`, e);
    return false;
  }
}

// ============================================================================
// Import — zip extraction
// ============================================================================

/** Files the importer accepted/rejected. */
export interface ImportResult {
  success: boolean;
  files_imported: number;
  files_skipped: number;
}

const COMMON_ATTACHMENT_RE =
  /\.(png|jpg|jpeg|gif|svg|pdf|webp|heic|heif|mp3|mp4|wav|mov|docx?|xlsx?|pptx?)$/i;

function isHiddenOrSystemSegment(part: string): boolean {
  return (
    part.startsWith('.') ||
    part === '__MACOSX' ||
    part === 'Thumbs.db' ||
    part === 'desktop.ini'
  );
}

function shouldSkipZipPath(path: string): boolean {
  return path
    .split('/')
    .some((part) => isHiddenOrSystemSegment(part));
}

function detectCommonRootPrefix(
  zip: JSZipType,
  fileNames: string[],
): string {
  const candidates = fileNames
    .filter((name) => !zip.files[name].dir)
    .filter((name) => !shouldSkipZipPath(name));

  if (candidates.length === 0) {
    return '';
  }

  let sharedRoot: string | null = null;
  for (const name of candidates) {
    const firstSlash = name.indexOf('/');
    if (firstSlash <= 0) {
      return '';
    }
    const root = name.substring(0, firstSlash);
    if (sharedRoot === null) {
      sharedRoot = root;
      continue;
    }
    if (sharedRoot !== root) {
      return '';
    }
  }

  return sharedRoot ? `${sharedRoot}/` : '';
}

/**
 * Extract files from a zip into the workspace via `writer` callbacks.
 *
 * - Strips a common root folder prefix when all files share one.
 * - Skips hidden files, system files, and unsupported file types.
 * - Markdown files are written as text; recognised binary attachments as binary.
 *
 * @param zip      An already-loaded JSZip instance.
 * @param workspace  The workspace directory prefix (e.g. `"."`).
 * @param writer   Callbacks to actually persist the files.
 * @param onProgress Optional progress callback.
 */
export async function importFilesFromZip(
  zip: JSZipType,
  workspace: string,
  writer: ImportFileWriter,
  onProgress?: (done: number, total: number) => void,
): Promise<ImportResult> {
  const fileNames = Object.keys(zip.files);
  const totalFiles = fileNames.length;
  let filesImported = 0;
  let filesSkipped = 0;

  // Detect common root directory prefix to strip.
  // Ignore hidden/system entries so "__MACOSX" or dotfiles don't break detection.
  const commonPrefix = detectCommonRootPrefix(zip, fileNames);

  for (let i = 0; i < fileNames.length; i++) {
    let fileName = fileNames[i];
    const zipEntry = zip.files[fileName];

    // Strip common root prefix
    if (commonPrefix && fileName.startsWith(commonPrefix)) {
      fileName = fileName.substring(commonPrefix.length);
      if (fileName === '') continue;
    }

    // Skip directories
    if (zipEntry.dir) continue;

    // Skip hidden / system files
    const shouldSkip = shouldSkipZipPath(fileName);
    if (shouldSkip) {
      filesSkipped++;
      continue;
    }

    const isMarkdown = fileName.endsWith('.md');
    const isAttachment = COMMON_ATTACHMENT_RE.test(fileName);

    if (!isMarkdown && !isAttachment) {
      filesSkipped++;
      continue;
    }

    const filePath = `${workspace}/${fileName}`;

    try {
      if (!isMarkdown) {
        const data = await zipEntry.async('uint8array');
        await writer.writeBinary(filePath, data);
      } else {
        const content = await zipEntry.async('string');
        await writer.writeText(filePath, content);
      }
      filesImported++;
    } catch {
      filesSkipped++;
    }

    onProgress?.(i + 1, totalFiles);
  }

  return { success: true, files_imported: filesImported, files_skipped: filesSkipped };
}
