import { describe, expect, it } from "vitest";

import type { TreeNode } from "./backend";
import {
  collectInitiallyExpandedMovePaths,
  collectMoveDisabledPaths,
  collectMoveMatchingPaths,
  computeMoveDialogAction,
  findMoveParentNode,
  highlightMoveQueryMatch,
  isMoveNodeExpanded,
  isMoveNodeVisible,
} from "./moveEntryDialog";

const tree: TreeNode = {
  path: "README.md",
  name: "README.md",
  description: null,
  is_index: true,
  children: [
    {
      path: "projects",
      name: "projects",
      description: null,
      is_index: false,
      children: [
        {
          path: "projects/alpha.md",
          name: "alpha.md",
          description: null,
          is_index: false,
          children: [],
        },
        {
          path: "projects/beta.md",
          name: "beta.md",
          description: null,
          is_index: false,
          children: [],
        },
      ],
    },
    {
      path: "journal",
      name: "journal",
      description: null,
      is_index: false,
      children: [
        {
          path: "journal/day-one.md",
          name: "day-one.md",
          description: null,
          is_index: false,
          children: [],
        },
      ],
    },
  ],
};

describe("moveEntryDialog helpers", () => {
  it("collects descendants of the entry being moved as disabled paths", () => {
    expect(Array.from(collectMoveDisabledPaths(tree, "projects"))).toEqual([
      "projects",
      "projects/alpha.md",
      "projects/beta.md",
    ]);
  });

  it("collects search matches and ancestors", () => {
    const matching = collectMoveMatchingPaths(tree, "day");
    expect(matching?.matches).toEqual(new Set(["journal/day-one.md"]));
    expect(matching?.ancestors).toEqual(new Set(["README.md", "journal"]));
    expect(collectMoveMatchingPaths(tree, "   ")).toBeNull();
  });

  it("collects the branch that should start expanded", () => {
    expect(Array.from(collectInitiallyExpandedMovePaths(tree, "journal/day-one.md"))).toEqual([
      "README.md",
      "journal",
    ]);
  });

  it("finds parent nodes and computes same-parent reorder operations", () => {
    expect(findMoveParentNode(tree, "projects/alpha.md")?.path).toBe("projects");

    expect(
      computeMoveDialogAction(tree, "projects/alpha.md", {
        path: "projects/beta.md",
        position: "below",
      }),
    ).toEqual({
      type: "reorder",
      parentPath: "projects",
      childPaths: ["projects/beta.md", "projects/alpha.md"],
    });
  });

  it("computes cross-parent moves with before/after hints", () => {
    expect(
      computeMoveDialogAction(tree, "projects/alpha.md", {
        path: "journal/day-one.md",
        position: "above",
      }),
    ).toEqual({
      type: "move",
      targetPath: "journal",
      position: {
        beforePath: "journal/day-one.md",
        afterPath: undefined,
      },
    });

    expect(
      computeMoveDialogAction(tree, "projects/alpha.md", {
        path: "journal",
        position: "on",
      }),
    ).toEqual({
      type: "move",
      targetPath: "journal",
    });
  });

  it("handles visibility, expansion, and highlighted match markup", () => {
    const matching = collectMoveMatchingPaths(tree, "alpha");
    expect(isMoveNodeVisible("projects", matching)).toBe(true);
    expect(isMoveNodeVisible("journal", matching)).toBe(false);
    expect(isMoveNodeExpanded("projects", new Set(), matching)).toBe(true);
    expect(isMoveNodeExpanded("journal", new Set(["journal"]), null)).toBe(true);
    expect(highlightMoveQueryMatch("Alpha note", "pha")).toBe(
      'Al<mark class="bg-yellow-200 dark:bg-yellow-800 rounded-sm">pha</mark> note',
    );
  });
});
