import type { TreeNode } from "$lib/backend/interface";

const IGNORED_TOP_LEVEL_NAMES = new Set([".diaryx", "guest"]);

function hasMeaningfulContent(node: TreeNode, depth: number): boolean {
  if (depth === 1 && IGNORED_TOP_LEVEL_NAMES.has(node.name)) {
    return false;
  }

  if (!Array.isArray(node.children) || node.children.length === 0) {
    return depth > 0;
  }

  return node.children.some((child) => hasMeaningfulContent(child, depth + 1));
}

export function isEffectivelyEmptyWorkspaceTree(tree: TreeNode | null): boolean {
  if (!tree) return true;
  if (!Array.isArray(tree.children) || tree.children.length === 0) {
    return true;
  }

  return !tree.children.some((child) => hasMeaningfulContent(child, 1));
}
