export interface FilesystemTreeNodeLike {
  path?: string | null;
  children?: FilesystemTreeNodeLike[] | null;
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/");
}

function isIndexFilePath(path: string): boolean {
  return /(^|\/)(README|index)\.md$/i.test(path) || /(^|\/)[^/]+\.index\.md$/i.test(path);
}

function looksLikeFilePath(path: string): boolean {
  return /(^|\/)[^/]+\.[^/]+$/.test(path);
}

export function collectFilesystemTreePaths(
  node: FilesystemTreeNodeLike | null | undefined,
): string[] {
  if (!node) return [];

  const files = new Set<string>();

  const walk = (current: FilesystemTreeNodeLike) => {
    const path = typeof current.path === "string" ? normalizePath(current.path) : "";
    const children = Array.isArray(current.children) ? current.children : [];

    if (path) {
      if (isIndexFilePath(path) || (children.length === 0 && looksLikeFilePath(path))) {
        files.add(path);
      }
    }

    for (const child of children) {
      walk(child);
    }
  };

  walk(node);
  return Array.from(files);
}
