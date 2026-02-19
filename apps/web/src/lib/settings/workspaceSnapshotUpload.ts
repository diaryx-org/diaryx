import type { Api } from "$lib/backend/api";
import type { Backend } from "$lib/backend/interface";

export interface WorkspaceSnapshotBuildProgress {
  completedFiles: number;
  totalFiles: number;
  phase: "scan" | "zip";
  detail?: string;
}

export interface WorkspaceSnapshotBuildResult {
  blob: Blob;
  filesPlanned: number;
  filesAdded: number;
  filesSkipped: number;
  attachmentReadFailures: number;
}

function normalizeZipPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/^\.\/+/, "").replace(/^\/+/, "");
  return normalized;
}

export function resolveWorkspaceDir(backend: Backend): string {
  return backend
    .getWorkspacePath()
    .replace(/\/index\.md$/, "")
    .replace(/\/README\.md$/, "");
}

export async function findWorkspaceRootPath(
  api: Api,
  backend: Backend,
): Promise<string | null> {
  const workspaceDir = resolveWorkspaceDir(backend);
  try {
    return await api.findRootIndex(workspaceDir);
  } catch {
    return null;
  }
}

export async function buildWorkspaceSnapshotUploadBlob(
  api: Api,
  workspaceRootPath: string,
  onProgress?: (progress: WorkspaceSnapshotBuildProgress) => void,
): Promise<WorkspaceSnapshotBuildResult> {
  onProgress?.({
    phase: "scan",
    completedFiles: 0,
    totalFiles: 0,
    detail: "Planning workspace export...",
  });

  const [plan, binaries] = await Promise.all([
    api.planExport(workspaceRootPath, "*"),
    api.exportBinaryAttachments(workspaceRootPath, "*"),
  ]);

  const totalFiles = plan.included.length + binaries.length;

  onProgress?.({
    phase: "scan",
    completedFiles: 0,
    totalFiles,
    detail: `${totalFiles} file${totalFiles === 1 ? "" : "s"} planned`,
  });

  const { ZipWriter, BlobWriter, TextReader, Uint8ArrayReader } = await import("@zip.js/zip.js");
  const zipWriter = new ZipWriter(new BlobWriter("application/zip"));

  let completedFiles = 0;
  let filesSkipped = 0;
  let attachmentReadFailures = 0;

  for (const file of plan.included) {
    const zipPath = normalizeZipPath(file.relative_path);
    if (!zipPath) {
      filesSkipped += 1;
      continue;
    }

    const content = await api.readFile(file.source_path);
    await zipWriter.add(zipPath, new TextReader(content));

    completedFiles += 1;
    onProgress?.({
      phase: "zip",
      completedFiles,
      totalFiles,
      detail: zipPath,
    });
  }

  for (const attachment of binaries) {
    const zipPath = normalizeZipPath(attachment.relative_path);
    if (!zipPath) {
      filesSkipped += 1;
      continue;
    }

    try {
      const data = await api.readBinary(attachment.source_path);
      await zipWriter.add(zipPath, new Uint8ArrayReader(data));
      completedFiles += 1;
    } catch (e) {
      attachmentReadFailures += 1;
      filesSkipped += 1;
      console.warn(
        `[SnapshotUpload] Failed to include attachment ${attachment.source_path}:`,
        e,
      );
    }

    onProgress?.({
      phase: "zip",
      completedFiles,
      totalFiles,
      detail: zipPath,
    });
  }

  const blob = await zipWriter.close();
  return {
    blob,
    filesPlanned: totalFiles,
    filesAdded: completedFiles,
    filesSkipped,
    attachmentReadFailures,
  };
}
