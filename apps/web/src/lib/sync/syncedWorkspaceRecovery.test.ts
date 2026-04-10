import { describe, expect, it } from "vitest";
import { isEffectivelyEmptyWorkspaceTree } from "./syncedWorkspaceRecovery";

describe("syncedWorkspaceRecovery", () => {
  it("treats an empty workspace tree as recoverable", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree({
        name: "workspace",
        description: null,
        path: "/workspace",
        is_index: false,
        audience: [],
        children: [],
      }),
    ).toBe(true);
  });

  it("ignores plugin/system files when checking for empty workspaces", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree({
        name: "workspace",
        description: null,
        path: "/workspace",
        is_index: false,
        audience: [],
        children: [{
          name: ".diaryx",
          description: null,
          path: "/workspace/.diaryx",
          is_index: false,
          audience: [],
          children: [{
            name: "plugins",
            description: null,
            path: "/workspace/.diaryx/plugins",
            is_index: false,
            audience: [],
            children: [],
          }],
        }],
      }),
    ).toBe(true);
  });

  it("detects real workspace content outside ignored directories", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree({
        name: "workspace",
        description: null,
        path: "/workspace",
        is_index: false,
        audience: [],
        children: [{
          name: "index.md",
          description: null,
          path: "/workspace/index.md",
          is_index: false,
          audience: [],
          children: [],
        }],
      }),
    ).toBe(false);
  });
});
