import { describe, expect, it } from "vitest";
import { isEffectivelyEmptyWorkspaceTree } from "./syncedWorkspaceRecovery";

describe("syncedWorkspaceRecovery", () => {
  it("treats an empty workspace tree as recoverable", () => {
    expect(
      isEffectivelyEmptyWorkspaceTree({
        name: "workspace",
        description: null,
        path: "/workspace",
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
        children: [{
          name: ".diaryx",
          description: null,
          path: "/workspace/.diaryx",
          children: [{
            name: "plugins",
            description: null,
            path: "/workspace/.diaryx/plugins",
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
        children: [{
          name: "index.md",
          description: null,
          path: "/workspace/index.md",
          children: [],
        }],
      }),
    ).toBe(false);
  });
});
