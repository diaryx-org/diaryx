import type { TreeNode } from "$lib/backend";

export interface FilePickerEntry {
  path: string;
  name: string;
}

export function collectUniqueEntries(node: TreeNode | null): FilePickerEntry[] {
  if (!node) return [];

  const entries: FilePickerEntry[] = [];
  const seenPaths = new Set<string>();

  function traverse(current: TreeNode) {
    if (!seenPaths.has(current.path)) {
      seenPaths.add(current.path);
      entries.push({ path: current.path, name: current.name });
    }

    for (const child of current.children) {
      traverse(child);
    }
  }

  traverse(node);
  return entries;
}

export function filterEntries(
  entries: FilePickerEntry[],
  searchValue: string,
  excludePaths: string[] = [],
): FilePickerEntry[] {
  const normalizedSearch = searchValue.trim().toLowerCase();
  const excluded = new Set(excludePaths);

  return entries.filter((entry) => {
    if (excluded.has(entry.path)) return false;
    if (!normalizedSearch) return true;

    return (
      entry.name.toLowerCase().includes(normalizedSearch) ||
      entry.path.toLowerCase().includes(normalizedSearch)
    );
  });
}
