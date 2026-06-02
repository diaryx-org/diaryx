import { beforeEach, describe, expect, it, vi } from "vitest";

async function loadWorkspaceAccess(options?: {
  isTauri?: boolean;
  isIOS?: boolean;
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
  vi.doMock("$lib/hooks/useMobile.svelte", () => ({
    isIOS: () => options?.isIOS ?? false,
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

  it("uses the native authorized picker command on iOS", async () => {
    const { pickAuthorizedWorkspaceFolder, open, invoke } =
      await loadWorkspaceAccess({
        isIOS: true,
        authorizedPath: "/ios/resolved/workspace",
      });

    await expect(
      pickAuthorizedWorkspaceFolder("Create Workspace in Folder"),
    ).resolves.toBe("/ios/resolved/workspace");
    expect(open).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("pick_authorized_workspace_folder", {
      title: "Create Workspace in Folder",
    });
  });

  it("authorizes the selected file from the native picker", async () => {
    const { pickAuthorizedWorkspaceFile, open, invoke } =
      await loadWorkspaceAccess({
        selection: "/picked/root.md",
        authorizedPath: "/resolved/root.md",
      });

    await expect(
      pickAuthorizedWorkspaceFile("Open Markdown File"),
    ).resolves.toBe("/resolved/root.md");
    expect(open).toHaveBeenCalledWith({
      title: "Open Markdown File",
      filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
    });
    expect(invoke).toHaveBeenCalledWith("authorize_workspace_path", {
      workspacePath: "/picked/root.md",
    });
  });

  it("uses the native authorized file picker command on iOS", async () => {
    const { pickAuthorizedWorkspaceFile, open, invoke } =
      await loadWorkspaceAccess({
        isIOS: true,
        authorizedPath: "/ios/resolved/root.md",
      });

    await expect(
      pickAuthorizedWorkspaceFile("Open Root File"),
    ).resolves.toBe("/ios/resolved/root.md");
    expect(open).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("pick_authorized_workspace_file", {
      title: "Open Root File",
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
