import { describe, expect, it } from "vitest";

import type { TreeNode } from "./backend";
import {
  collectVisibleTreePaths,
  expandDeleteSelection,
  getTreeSelectionRange,
  orderDeletePaths,
  pruneNestedDeleteRoots,
  selectionIncludesDescendants,
} from "./leftSidebarSelection";

function createNode(
  path: string,
  children: TreeNode[] = [],
  name = path.split("/").pop() ?? path,
): TreeNode {
  return {
    name,
    description: null,
    path,
    is_index: false,
    children,
  };
}

const tree = createNode("README.md", [
  createNode("alpha.md"),
  createNode("section/index.md", [
    createNode("section/child.md"),
    createNode("shared/nested.md"),
  ]),
  createNode("omega.md"),
]);

describe("leftSidebarSelection", () => {
  it("collects visible paths in rendered tree order", () => {
    const expandedNodes = new Set(["README.md", "section/index.md"]);

    expect(collectVisibleTreePaths(tree, expandedNodes)).toEqual([
      "README.md",
      "alpha.md",
      "section/index.md",
      "section/child.md",
      "shared/nested.md",
      "omega.md",
    ]);
  });

  it("builds an inclusive range selection from the anchor", () => {
    const visiblePaths = [
      "README.md",
      "alpha.md",
      "section/index.md",
      "section/child.md",
      "shared/nested.md",
      "omega.md",
    ];

    expect(
      getTreeSelectionRange(visiblePaths, "alpha.md", "shared/nested.md"),
    ).toEqual([
      "alpha.md",
      "section/index.md",
      "section/child.md",
      "shared/nested.md",
    ]);
  });

  it("prunes nested delete roots before expanding descendants", () => {
    expect(
      pruneNestedDeleteRoots(tree, [
        "section/index.md",
        "section/child.md",
        "omega.md",
      ]),
    ).toEqual(["section/index.md", "omega.md"]);
  });

  it("expands selected roots to include all descendants in the tree", () => {
    expect(expandDeleteSelection(tree, ["section/index.md"])).toEqual([
      "section/index.md",
      "section/child.md",
      "shared/nested.md",
    ]);
  });

  it("orders deletes from descendants to ancestors", () => {
    expect(
      orderDeletePaths(tree, [
        "README.md",
        "section/index.md",
        "section/child.md",
        "shared/nested.md",
      ]),
    ).toEqual([
      "section/child.md",
      "shared/nested.md",
      "section/index.md",
      "README.md",
    ]);
  });

  it("detects when a selection will also remove descendants", () => {
    expect(selectionIncludesDescendants(tree, ["section/index.md"])).toBe(true);
    expect(selectionIncludesDescendants(tree, ["omega.md"])).toBe(false);
  });
});
