import { describe, expect, it } from "vitest";
import type { TreeNode } from "$lib/backend";
import { isEffectivelyEmptyWorkspaceTree } from "./syncedWorkspaceRecovery";

function node(overrides: Partial<TreeNode> & Pick<TreeNode, "name" | "path">): TreeNode {
  return {
    description: null,
    is_index: false,
    audience: [],
    children: [],
    properties: undefined,
    ...overrides,
  };
}

describe("syncedWorkspaceRecovery", () => {
  it("treats an empty workspace tree as recoverable", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree(
        node({ name: "workspace", path: "/workspace" }),
      ),
    ).toBe(true);
  });

  it("ignores plugin/system files when checking for empty workspaces", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree(
        node({
          name: "workspace",
          path: "/workspace",
          children: [
            node({
              name: ".diaryx",
              path: "/workspace/.diaryx",
              children: [
                node({ name: "plugins", path: "/workspace/.diaryx/plugins" }),
              ],
            }),
          ],
        }),
      ),
    ).toBe(true);
  });

  it("detects real workspace content outside ignored directories", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree(
        node({
          name: "workspace",
          path: "/workspace",
          children: [
            node({ name: "index.md", path: "/workspace/index.md" }),
          ],
        }),
      ),
    ).toBe(false);
  });
});
