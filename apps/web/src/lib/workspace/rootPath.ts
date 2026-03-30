import type { Api } from "$lib/backend";
import type { TreeNode } from "$lib/backend/interface";

function normalizeWorkspacePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/$/, "");
}

function isMarkdownWorkspacePath(path: string): boolean {
  const lastSegment = path.split("/").pop() ?? "";
  return /\.(md|markdown)$/i.test(lastSegment);
}

export function getWorkspaceDirectoryPath(path: string): string {
  const normalizedPath = normalizeWorkspacePath(path);
  if (!normalizedPath || !isMarkdownWorkspacePath(normalizedPath)) {
    return normalizedPath || path;
  }

  const lastSlash = normalizedPath.lastIndexOf("/");
  if (lastSlash < 0) {
    return ".";
  }

  return normalizedPath.slice(0, lastSlash) || "/";
}

export async function resolveWorkspaceValidationRootPath(
  api: Api,
  tree: TreeNode | null,
  backendWorkspacePath: string,
): Promise<string> {
  if (tree?.path) {
    return tree.path;
  }

  return (
    (await api.resolveWorkspaceRootIndexPath(backendWorkspacePath)) ??
    backendWorkspacePath
  );
}
