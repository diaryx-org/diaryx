import { importFilesFromZipBlob } from "$lib/settings/zipUtils";

export type LocalPluginSelection = File | FileSystemDirectoryHandle;

type WorkspaceImportWriter = {
  writeText: (path: string, content: string) => Promise<void>;
  writeBinary: (path: string, data: Uint8Array) => Promise<void>;
};

const SKIP_DIRS = new Set([
  "node_modules", "target", ".git", ".svn", "dist", "build",
  "__pycache__", ".next", ".nuxt", "vendor", ".cargo", ".obsidian",
  ".trash", ".diaryx",
]);

const selections = new Map<string, LocalPluginSelection>();

function createSelectionToken(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `plugin-selection-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function storePluginLocalSelection(selection: LocalPluginSelection): string {
  const token = createSelectionToken();
  selections.set(token, selection);
  return token;
}

export function getPluginLocalSelection(token: string): LocalPluginSelection | null {
  return selections.get(token) ?? null;
}

export async function readPluginLocalSelectionFile(token: string): Promise<Uint8Array | null> {
  const selection = selections.get(token);
  if (!(selection instanceof File)) {
    return null;
  }
  return new Uint8Array(await selection.arrayBuffer());
}

async function readDirectoryRecursively(
  handle: FileSystemDirectoryHandle,
  prefix = "",
): Promise<Array<{ relativePath: string; file: File }>> {
  const results: Array<{ relativePath: string; file: File }> = [];

  for await (const [name, entry] of (handle as any).entries()) {
    if (name.startsWith(".")) continue;

    if (entry.kind === "directory") {
      if (SKIP_DIRS.has(name)) continue;
      results.push(...await readDirectoryRecursively(
        entry as FileSystemDirectoryHandle,
        prefix ? `${prefix}/${name}` : name,
      ));
      continue;
    }

    const file = await (entry as FileSystemFileHandle).getFile();
    results.push({ relativePath: prefix ? `${prefix}/${name}` : name, file });
  }

  return results;
}

export async function importPluginLocalSelectionToWorkspace(
  selection: LocalPluginSelection,
  destinationPrefix: string,
  writer: WorkspaceImportWriter,
): Promise<{ files_imported: number; files_skipped: number; errors: string[] }> {
  if (selection instanceof File) {
    const result = await importFilesFromZipBlob(selection, destinationPrefix || ".", writer);
    return {
      files_imported: result.files_imported,
      files_skipped: result.files_skipped,
      errors: [],
    };
  }

  const entries = await readDirectoryRecursively(selection);
  let filesImported = 0;
  let filesSkipped = 0;
  const errors: string[] = [];

  for (const { relativePath, file } of entries) {
    const destPath = destinationPrefix ? `${destinationPrefix}/${relativePath}` : relativePath;

    try {
      if (/\.md$/i.test(file.name)) {
        await writer.writeText(destPath, await file.text());
      } else {
        await writer.writeBinary(destPath, new Uint8Array(await file.arrayBuffer()));
      }
      filesImported += 1;
    } catch (error) {
      errors.push(`${relativePath}: ${error instanceof Error ? error.message : String(error)}`);
      filesSkipped += 1;
    }
  }

  return { files_imported: filesImported, files_skipped: filesSkipped, errors };
}
