import type { TreeNode } from "./backend";

function isPlaceholderNode(node: TreeNode): boolean {
  return node.name.startsWith("... (");
}

function comparePathDepthDescending(a: string, b: string): number {
  const aDepth = a.split("/").length;
  const bDepth = b.split("/").length;
  if (aDepth !== bDepth) return bDepth - aDepth;
  return b.localeCompare(a);
}

export function getRenderableSidebarChildren(node: TreeNode): TreeNode[] {
  const seen = new Set<string>();
  const children: TreeNode[] = [];

  for (const child of node.children) {
    if (isPlaceholderNode(child)) continue;
    if (seen.has(child.path)) continue;
    seen.add(child.path);
    children.push(child);
  }

  return children;
}

export function hasUnloadedSidebarChildren(node: TreeNode): boolean {
  return node.children.some(isPlaceholderNode);
}

export function findTreeNode(
  tree: TreeNode | null,
  path: string,
): TreeNode | null {
  if (!tree) return null;
  if (tree.path === path) return tree;

  for (const child of getRenderableSidebarChildren(tree)) {
    const match = findTreeNode(child, path);
    if (match) return match;
  }

  return null;
}

export function collectTreePaths(tree: TreeNode | null): string[] {
  if (!tree) return [];

  const paths: string[] = [];

  function visit(node: TreeNode) {
    paths.push(node.path);
    for (const child of getRenderableSidebarChildren(node)) {
      visit(child);
    }
  }

  visit(tree);
  return paths;
}

export function collectVisibleTreePaths(
  tree: TreeNode | null,
  expandedNodes: Set<string>,
): string[] {
  if (!tree) return [];

  const paths: string[] = [];

  function visit(node: TreeNode) {
    paths.push(node.path);
    if (!expandedNodes.has(node.path)) return;

    for (const child of getRenderableSidebarChildren(node)) {
      visit(child);
    }
  }

  visit(tree);
  return paths;
}

export function getTreeSelectionRange(
  visiblePaths: string[],
  anchorPath: string | null,
  targetPath: string,
): string[] {
  if (!anchorPath) return [targetPath];

  const anchorIndex = visiblePaths.indexOf(anchorPath);
  const targetIndex = visiblePaths.indexOf(targetPath);
  if (anchorIndex === -1 || targetIndex === -1) {
    return [targetPath];
  }

  const start = Math.min(anchorIndex, targetIndex);
  const end = Math.max(anchorIndex, targetIndex);
  return visiblePaths.slice(start, end + 1);
}

export function pruneNestedDeleteRoots(
  tree: TreeNode | null,
  paths: Iterable<string>,
): string[] {
  const selected = new Set(paths);
  const roots: string[] = [];
  const covered = new Set<string>();

  if (tree) {
    function visit(node: TreeNode, ancestorSelected: boolean) {
      const isSelected = selected.has(node.path);
      if (isSelected) {
        covered.add(node.path);
      }
      if (isSelected && !ancestorSelected) {
        roots.push(node.path);
      }

      const nextAncestorSelected = ancestorSelected || isSelected;
      for (const child of getRenderableSidebarChildren(node)) {
        visit(child, nextAncestorSelected);
      }
    }

    visit(tree, false);
  }

  const remaining = Array.from(selected)
    .filter((path) => !covered.has(path))
    .sort(comparePathDepthDescending);

  return [...roots, ...remaining];
}

export function expandDeleteSelection(
  tree: TreeNode | null,
  rootPaths: Iterable<string>,
): string[] {
  const roots = new Set(rootPaths);
  const expanded = new Set<string>();

  if (tree) {
    function visit(node: TreeNode, ancestorSelected: boolean) {
      const isSelected = ancestorSelected || roots.has(node.path);
      if (isSelected) {
        expanded.add(node.path);
      }

      for (const child of getRenderableSidebarChildren(node)) {
        visit(child, isSelected);
      }
    }

    visit(tree, false);
  }

  for (const path of roots) {
    expanded.add(path);
  }

  return Array.from(expanded);
}

export function orderDeletePaths(
  tree: TreeNode | null,
  paths: Iterable<string>,
): string[] {
  const selected = new Set(paths);
  const ordered: string[] = [];
  const seen = new Set<string>();

  if (tree) {
    function visit(node: TreeNode) {
      for (const child of getRenderableSidebarChildren(node)) {
        visit(child);
      }

      if (selected.has(node.path) && !seen.has(node.path)) {
        ordered.push(node.path);
        seen.add(node.path);
      }
    }

    visit(tree);
  }

  const remaining = Array.from(selected)
    .filter((path) => !seen.has(path))
    .sort(comparePathDepthDescending);

  return [...ordered, ...remaining];
}

export function selectionIncludesDescendants(
  tree: TreeNode | null,
  rootPaths: Iterable<string>,
): boolean {
  const roots = pruneNestedDeleteRoots(tree, rootPaths);
  if (tree) {
    for (const rootPath of roots) {
      const node = findTreeNode(tree, rootPath);
      if (node && node.children.length > 0) {
        return true;
      }
    }
  }
  return expandDeleteSelection(tree, roots).length > roots.length;
}
