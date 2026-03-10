import type {
  StarterWorkspaceRegistryEntry,
  StarterWorkspaceFile,
} from "./types";

export interface StarterWorkspaceManifest {
  files: StarterWorkspaceFile[];
}

export interface StarterApplyProgress {
  percent: number;
  message: string;
}

export interface StarterApplyRuntime {
  createWorkspace(path: string, name: string): Promise<void>;
  findRootIndex(workspaceDir: string): Promise<string>;
  saveEntry(path: string, content: string, rootIndexPath?: string): Promise<string | null>;
  saveTemplate(name: string, content: string, workspacePath: string): Promise<void>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function parseStarterWorkspaceManifest(payload: unknown): StarterWorkspaceManifest {
  if (!isRecord(payload)) {
    throw new Error("Starter workspace manifest must be an object");
  }

  const filesRaw = payload.files;
  if (!Array.isArray(filesRaw)) {
    throw new Error("Starter workspace manifest 'files' must be an array");
  }

  const files: StarterWorkspaceFile[] = filesRaw.map((file, index) => {
    if (!isRecord(file)) {
      throw new Error(`Starter workspace manifest file[${index}] must be an object`);
    }

    const path = file.path;
    if (typeof path !== "string" || path.length === 0) {
      throw new Error(`Starter workspace manifest file[${index}] 'path' must be a non-empty string`);
    }

    const content = file.content;
    if (typeof content !== "string") {
      throw new Error(`Starter workspace manifest file[${index}] 'content' must be a string`);
    }

    return { path, content };
  });

  return { files };
}

export async function fetchStarterWorkspaceManifest(
  entry: StarterWorkspaceRegistryEntry,
): Promise<StarterWorkspaceManifest> {
  if (!entry.artifact) {
    throw new Error(`Starter workspace '${entry.id}' has no artifact`);
  }

  const resp = await fetch(entry.artifact.url);
  if (!resp.ok) {
    throw new Error(`Failed to fetch starter workspace artifact: ${resp.status}`);
  }

  const payload = await resp.json();
  return parseStarterWorkspaceManifest(payload);
}

function isTemplatePath(path: string): boolean {
  return path.startsWith("_templates/") || path.startsWith("_templates\\");
}

function templateNameFromPath(path: string): string {
  const filename = path.split("/").pop() ?? path.split("\\").pop() ?? path;
  return filename.replace(/\.md$/, "");
}

function sortFilesByDepth(files: StarterWorkspaceFile[]): StarterWorkspaceFile[] {
  return [...files].sort((a, b) => {
    const depthA = a.path.split("/").length;
    const depthB = b.path.split("/").length;
    return depthA - depthB;
  });
}

export async function applyStarterWorkspace(
  manifest: StarterWorkspaceManifest,
  workspaceDir: string,
  runtime: StarterApplyRuntime,
  onProgress?: (progress: StarterApplyProgress) => void,
): Promise<{ filesCreated: number; templatesCreated: number }> {
  const sorted = sortFilesByDepth(manifest.files);
  const total = sorted.length;
  let filesCreated = 0;
  let templatesCreated = 0;

  let rootIndexPath: string | undefined;
  try {
    rootIndexPath = await runtime.findRootIndex(workspaceDir);
  } catch {
    // Root index may not exist yet; will be created by the first file write
  }

  for (let i = 0; i < sorted.length; i++) {
    const file = sorted[i];
    const percent = Math.round(((i + 1) / total) * 100);

    if (isTemplatePath(file.path)) {
      const templateName = templateNameFromPath(file.path);
      onProgress?.({
        percent,
        message: `Creating template "${templateName}"...`,
      });
      await runtime.saveTemplate(templateName, file.content, workspaceDir);
      templatesCreated++;
    } else {
      const fullPath = workspaceDir.replace(/\/$/, "") + "/" + file.path;
      onProgress?.({
        percent,
        message: `Creating "${file.path}"...`,
      });
      await runtime.saveEntry(fullPath, file.content, rootIndexPath);
      filesCreated++;

      // If we just created the root index, capture its path
      if (!rootIndexPath && (file.path === "index.md" || file.path === "README.md")) {
        try {
          rootIndexPath = await runtime.findRootIndex(workspaceDir);
        } catch {
          // Not critical
        }
      }
    }
  }

  onProgress?.({ percent: 100, message: "Done." });

  return { filesCreated, templatesCreated };
}
