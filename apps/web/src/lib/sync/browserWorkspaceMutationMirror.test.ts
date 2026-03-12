import { beforeEach, describe, expect, it, vi } from "vitest";

const registryMocks = vi.hoisted(() => ({
  getCurrentWorkspaceId: vi.fn(),
  getLocalWorkspace: vi.fn(),
  getWorkspaceProviderLinks: vi.fn(),
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => registryMocks);

import { mirrorCurrentWorkspaceMutationToLinkedProviders } from "./browserWorkspaceMutationMirror";

describe("browserWorkspaceMutationMirror", () => {
  const runPluginCommand = vi.fn<
    (pluginId: string, command: string, params?: unknown) => Promise<unknown>
  >(async () => ({}));
  const backend = {
    getWorkspacePath: vi.fn(() => "."),
    resolveRootIndex: vi.fn(async (path: string) => path),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    backend.getWorkspacePath.mockReturnValue(".");
    backend.resolveRootIndex.mockImplementation(async (path: string) => path);
    registryMocks.getCurrentWorkspaceId.mockReturnValue("local-1");
    registryMocks.getLocalWorkspace.mockReturnValue({
      id: "local-1",
      name: "Journal",
      path: "/tmp/journal/README.md",
    });
    registryMocks.getWorkspaceProviderLinks.mockReturnValue([
      {
        pluginId: "diaryx.backup",
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      },
    ]);
  });

  it("reinitializes and syncs all linked providers with live sync enabled", async () => {
    registryMocks.getWorkspaceProviderLinks.mockReturnValue([
      {
        pluginId: "diaryx.backup",
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      },
      {
        pluginId: "diaryx.share",
        remoteWorkspaceId: "remote-2",
        syncEnabled: false,
      },
    ]);

    await mirrorCurrentWorkspaceMutationToLinkedProviders({
      backend,
      runPluginCommand,
    });

    expect(runPluginCommand).toHaveBeenCalledTimes(2);
    expect(runPluginCommand).toHaveBeenNthCalledWith(
      1,
      "diaryx.backup",
      "InitializeWorkspaceCrdt",
      {
        provider_id: "diaryx.backup",
        workspace_path: "/tmp/journal/README.md",
      },
    );
    expect(runPluginCommand).toHaveBeenNthCalledWith(
      2,
      "diaryx.backup",
      "TriggerWorkspaceSync",
      {
        provider_id: "diaryx.backup",
      },
    );
  });

  it("falls back to the backend workspace path when the registry has no stored path", async () => {
    registryMocks.getLocalWorkspace.mockReturnValue({
      id: "local-1",
      name: "Journal",
      path: null,
    });
    backend.getWorkspacePath.mockReturnValue(".");
    backend.resolveRootIndex.mockResolvedValue("workspace/index.md");

    await mirrorCurrentWorkspaceMutationToLinkedProviders({
      backend,
      runPluginCommand,
    });

    expect(runPluginCommand).toHaveBeenNthCalledWith(
      1,
      "diaryx.backup",
      "InitializeWorkspaceCrdt",
      {
        provider_id: "diaryx.backup",
        workspace_path: "workspace/index.md",
      },
    );
  });

  it("continues after provider command failures", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    registryMocks.getWorkspaceProviderLinks.mockReturnValue([
      {
        pluginId: "diaryx.backup",
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      },
      {
        pluginId: "diaryx.share",
        remoteWorkspaceId: "remote-2",
        syncEnabled: true,
      },
    ]);

    runPluginCommand.mockImplementation(async (pluginId: string) => {
      if (pluginId === "diaryx.backup") {
        throw new Error("boom");
      }
      return {};
    });

    await expect(
      mirrorCurrentWorkspaceMutationToLinkedProviders({
        backend,
        runPluginCommand,
      }),
    ).resolves.toBeUndefined();

    expect(runPluginCommand).toHaveBeenCalledWith(
      "diaryx.share",
      "InitializeWorkspaceCrdt",
      {
        provider_id: "diaryx.share",
        workspace_path: "/tmp/journal/README.md",
      },
    );
    expect(warnSpy).toHaveBeenCalled();

    warnSpy.mockRestore();
  });

  it("falls back to the raw backend workspace path when root lookup fails", async () => {
    registryMocks.getLocalWorkspace.mockReturnValue({
      id: "local-1",
      name: "Journal",
      path: null,
    });
    backend.getWorkspacePath.mockReturnValue(".");
    backend.resolveRootIndex.mockRejectedValue(new Error("missing root"));

    await mirrorCurrentWorkspaceMutationToLinkedProviders({
      backend,
      runPluginCommand,
    });

    expect(runPluginCommand).toHaveBeenNthCalledWith(
      1,
      "diaryx.backup",
      "InitializeWorkspaceCrdt",
      {
        provider_id: "diaryx.backup",
        workspace_path: ".",
      },
    );
  });
});
