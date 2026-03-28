import type { TreeNode } from "./backend";
import { findTreeNode, getRenderableSidebarChildren } from "./leftSidebarSelection";

export interface MoveDialogHoverTarget {
  path: string;
  position: "above" | "below" | "on";
}

export interface MoveDialogMatchState {
  matches: Set<string>;
  ancestors: Set<string>;
}

export interface MoveDialogReorderResult {
  type: "reorder";
  parentPath: string;
  childPaths: string[];
}

export interface MoveDialogMoveResult {
  type: "move";
  targetPath: string;
  position?: { beforePath?: string; afterPath?: string };
}

export type MoveDialogActionResult = MoveDialogReorderResult | MoveDialogMoveResult | null;

export function collectMoveDisabledPaths(tree: TreeNode | null, entryPath: string): Set<string> {
  const paths = new Set<string>();
  if (!tree || !entryPath) return paths;

  const entryNode = findTreeNode(tree, entryPath);
  if (!entryNode) return paths;

  function collect(node: TreeNode) {
    paths.add(node.path);
    for (const child of getRenderableSidebarChildren(node)) {
      collect(child);
    }
  }

  collect(entryNode);
  return paths;
}

export function collectMoveMatchingPaths(
  tree: TreeNode | null,
  searchQuery: string,
): MoveDialogMatchState | null {
  if (!tree || !searchQuery.trim()) return null;

  const query = searchQuery.toLowerCase();
  const matches = new Set<string>();
  const ancestors = new Set<string>();

  function visit(node: TreeNode, parentChain: string[]) {
    const name = node.name.replace(".md", "").toLowerCase();
    if (name.includes(query)) {
      matches.add(node.path);
      for (const path of parentChain) {
        ancestors.add(path);
      }
    }

    for (const child of getRenderableSidebarChildren(node)) {
      visit(child, [...parentChain, node.path]);
    }
  }

  visit(tree, []);
  return { matches, ancestors };
}

export function collectInitiallyExpandedMovePaths(
  tree: TreeNode | null,
  entryPath: string,
): Set<string> {
  const expanded = new Set<string>();
  if (!tree || !entryPath) return expanded;

  function visit(node: TreeNode, chain: string[]): boolean {
    if (node.path === entryPath) {
      for (const path of chain) {
        expanded.add(path);
      }
      return true;
    }

    for (const child of getRenderableSidebarChildren(node)) {
      if (visit(child, [...chain, node.path])) {
        return true;
      }
    }

    return false;
  }

  visit(tree, []);
  return expanded;
}

export function findMoveParentNode(root: TreeNode, targetPath: string): TreeNode | null {
  for (const child of root.children) {
    if (child.path === targetPath) return root;
    const found = findMoveParentNode(child, targetPath);
    if (found) return found;
  }
  return null;
}

export function computeMoveDialogAction(
  tree: TreeNode | null,
  entryPath: string,
  hoverTarget: MoveDialogHoverTarget | null,
): MoveDialogActionResult {
  if (!tree || !hoverTarget) return null;

  const { path, position } = hoverTarget;

  if (position === "on") {
    return { type: "move", targetPath: path };
  }

  const targetParent = findMoveParentNode(tree, path);
  const sourceParent = findMoveParentNode(tree, entryPath);

  if (targetParent && sourceParent && targetParent.path === sourceParent.path) {
    const childPaths = getRenderableSidebarChildren(targetParent).map((child) => child.path);
    const fromIndex = childPaths.indexOf(entryPath);
    const toIndex = childPaths.indexOf(path);

    if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) {
      return null;
    }

    childPaths.splice(fromIndex, 1);
    const insertIndex = position === "below" ? childPaths.indexOf(path) + 1 : childPaths.indexOf(path);
    childPaths.splice(insertIndex, 0, entryPath);

    return {
      type: "reorder",
      parentPath: targetParent.path,
      childPaths,
    };
  }

  if (!targetParent) return null;

  return {
    type: "move",
    targetPath: targetParent.path,
    position: {
      beforePath: position === "above" ? path : undefined,
      afterPath: position === "below" ? path : undefined,
    },
  };
}

export function isMoveNodeVisible(nodePath: string, matchingPaths: MoveDialogMatchState | null): boolean {
  if (!matchingPaths) return true;
  return matchingPaths.matches.has(nodePath) || matchingPaths.ancestors.has(nodePath);
}

export function isMoveNodeExpanded(
  nodePath: string,
  expandedNodes: Set<string>,
  matchingPaths: MoveDialogMatchState | null,
): boolean {
  if (matchingPaths) {
    return matchingPaths.ancestors.has(nodePath) || matchingPaths.matches.has(nodePath);
  }
  return expandedNodes.has(nodePath);
}

export function highlightMoveQueryMatch(text: string, searchQuery: string): string {
  if (!searchQuery.trim()) return text;

  const query = searchQuery.toLowerCase();
  const lower = text.toLowerCase();
  const idx = lower.indexOf(query);

  if (idx === -1) return text;

  const before = text.slice(0, idx);
  const match = text.slice(idx, idx + query.length);
  const after = text.slice(idx + query.length);
  return `${before}<mark class="bg-yellow-200 dark:bg-yellow-800 rounded-sm">${match}</mark>${after}`;
}
