import { beforeEach, describe, expect, it, vi } from "vitest";

async function loadWorkspaceAccess(options?: {
  isTauri?: boolean;
  selection?: string | string[] | null;
  authorizedPath?: string;
}) {
  vi.resetModules();

  const invoke = vi
    .fn()
    .mockResolvedValue(options?.authorizedPath ?? "/authorized/workspace");
  const open = vi.fn().mockResolvedValue(options?.selection ?? null);

  vi.doMock("./interface", () => ({
    isTauri: () => options?.isTauri ?? true,
  }));
  vi.doMock("@tauri-apps/api/core", () => ({
    invoke,
  }));
  vi.doMock("@tauri-apps/plugin-dialog", () => ({
    open,
  }));

  const mod = await import("./workspaceAccess");
  return {
    ...mod,
    invoke,
    open,
  };
}

describe("workspaceAccess", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("passes through paths outside Tauri", async () => {
    const { authorizeWorkspacePath, invoke } = await loadWorkspaceAccess({
      isTauri: false,
    });

    await expect(authorizeWorkspacePath("  /raw/workspace  ")).resolves.toBe(
      "/raw/workspace",
    );
    expect(invoke).not.toHaveBeenCalled();
  });

  it("routes Tauri authorization through the native command", async () => {
    const { authorizeWorkspacePath, invoke } = await loadWorkspaceAccess({
      authorizedPath: "/resolved/workspace",
    });

    await expect(authorizeWorkspacePath(" /selected/workspace ")).resolves.toBe(
      "/resolved/workspace",
    );
    expect(invoke).toHaveBeenCalledWith("authorize_workspace_path", {
      workspacePath: "/selected/workspace",
    });
  });

  it("authorizes the selected folder from the native picker", async () => {
    const { pickAuthorizedWorkspaceFolder, open, invoke } =
      await loadWorkspaceAccess({
        selection: "/picked/workspace",
        authorizedPath: "/resolved/workspace",
      });

    await expect(
      pickAuthorizedWorkspaceFolder("Open Existing Folder"),
    ).resolves.toBe("/resolved/workspace");
    expect(open).toHaveBeenCalledWith({
      directory: true,
      title: "Open Existing Folder",
    });
    expect(invoke).toHaveBeenCalledWith("authorize_workspace_path", {
      workspacePath: "/picked/workspace",
    });
  });

  it("returns null when the user cancels the picker", async () => {
    const { pickAuthorizedWorkspaceFolder, invoke } = await loadWorkspaceAccess(
      {
        selection: null,
      },
    );

    await expect(
      pickAuthorizedWorkspaceFolder("Locate Workspace"),
    ).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
