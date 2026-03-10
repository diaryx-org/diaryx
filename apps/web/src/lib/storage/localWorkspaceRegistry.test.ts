import { beforeEach, describe, expect, it, vi } from "vitest";

type LocalWorkspaceRegistryModule = typeof import("./localWorkspaceRegistry.svelte");

async function loadRegistry(
  initialWorkspaces?: Array<Record<string, unknown>>,
): Promise<LocalWorkspaceRegistryModule> {
  vi.resetModules();
  localStorage.clear();

  if (initialWorkspaces) {
    localStorage.setItem(
      "diaryx_local_workspaces",
      JSON.stringify(initialWorkspaces),
    );
  }

  return await import("./localWorkspaceRegistry.svelte");
}

describe("localWorkspaceRegistry", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("normalizes legacy sync metadata into provider links", async () => {
    const registry = await loadRegistry([{
      id: "local-1",
      name: "Journal",
      isLocal: false,
      downloadedAt: 1,
      lastOpenedAt: 1,
      pluginMetadata: {
        sync: {
          serverId: "remote-1",
          syncEnabled: false,
        },
      },
    }]);

    expect(registry.getWorkspaceProviderLinks("local-1")).toEqual([{
      pluginId: "diaryx.sync",
      remoteWorkspaceId: "remote-1",
      syncEnabled: false,
    }]);
    expect(registry.getWorkspaceProviderLink("local-1", "diaryx.sync")).toEqual({
      pluginId: "diaryx.sync",
      remoteWorkspaceId: "remote-1",
      syncEnabled: false,
    });
    expect(registry.getPluginMetadata("local-1", "diaryx.sync")).toEqual({
      remoteWorkspaceId: "remote-1",
      serverId: "remote-1",
      syncEnabled: false,
    });
    expect(registry.getPrimaryWorkspaceProviderLink("local-1")).toEqual({
      pluginId: "diaryx.sync",
      remoteWorkspaceId: "remote-1",
      syncEnabled: false,
    });
  });

  it("persists normalized remote workspace IDs for provider metadata", async () => {
    const registry = await loadRegistry([{
      id: "local-1",
      name: "Journal",
      isLocal: true,
      downloadedAt: 1,
      lastOpenedAt: 1,
    }]);

    registry.setPluginMetadata("local-1", "acme.cloud", {
      serverId: "remote-9",
      syncEnabled: true,
    });

    expect(registry.getPluginMetadata("local-1", "acme.cloud")).toEqual({
      remoteWorkspaceId: "remote-9",
      serverId: "remote-9",
      syncEnabled: true,
    });
    expect(registry.getWorkspaceProviderLink("local-1", "acme.cloud")).toEqual({
      pluginId: "acme.cloud",
      remoteWorkspaceId: "remote-9",
      syncEnabled: true,
    });
    expect(registry.isWorkspaceSynced("local-1")).toBe(true);
    expect(registry.getServerWorkspaceId("local-1")).toBe("remote-9");
  });
});
