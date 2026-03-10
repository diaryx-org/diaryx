import { beforeEach, describe, expect, it, vi } from "vitest";

type MockBackend = {
  installPlugin: ReturnType<typeof vi.fn>;
  uninstallPlugin: ReturnType<typeof vi.fn>;
};

type MockPluginStore = {
  allManifests: Array<{ id: string; name?: string; ui: unknown[] }>;
  init: ReturnType<typeof vi.fn>;
  preloadInsertCommandIcons: ReturnType<typeof vi.fn>;
  clearPluginEnabled: ReturnType<typeof vi.fn>;
};

async function loadPluginInstallService() {
  vi.resetModules();

  const backend: MockBackend = {
    installPlugin: vi
      .fn()
      .mockResolvedValue(
        JSON.stringify({ id: "diaryx.spoiler", name: "Spoiler" }),
      ),
    uninstallPlugin: vi.fn().mockResolvedValue(undefined),
  };
  const pluginStore: MockPluginStore = {
    allManifests: [
      {
        id: "diaryx.spoiler",
        name: "Spoiler",
        ui: [],
      },
    ],
    init: vi.fn().mockResolvedValue(undefined),
    preloadInsertCommandIcons: vi.fn().mockResolvedValue(undefined),
    clearPluginEnabled: vi.fn(),
  };
  const api = { kind: "mock-api" };
  const createApi = vi.fn(() => api);
  const getBackend = vi.fn().mockResolvedValue(backend);
  const clearPreservedPluginEditorExtensions = vi.fn();
  const preservePluginEditorExtensions = vi.fn();

  vi.doMock("$lib/backend", () => ({
    getBackend,
    isTauri: () => true,
  }));
  vi.doMock("$lib/backend/api", () => ({
    createApi,
  }));
  vi.doMock("$lib/plugins/browserPluginManager.svelte", () => ({
    inspectPluginWasm: vi.fn(),
    installPlugin: vi.fn(),
    uninstallPlugin: vi.fn(),
  }));
  vi.doMock("$lib/plugins/preservedEditorExtensions.svelte", () => ({
    clearPreservedPluginEditorExtensions,
    preservePluginEditorExtensions,
  }));
  vi.doMock("@/models/stores/pluginStore.svelte", () => ({
    getPluginStore: () => pluginStore,
  }));
  vi.doMock("@/models/stores/workspaceStore.svelte", () => ({
    workspaceStore: { tree: null },
  }));

  const service = await import("./pluginInstallService");
  const installSource = await import("./pluginInstallSource.svelte");

  return {
    backend,
    clearPreservedPluginEditorExtensions,
    createApi,
    getBackend,
    installSource,
    pluginStore,
    preservePluginEditorExtensions,
    service,
  };
}

describe("pluginInstallService", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("refreshes frontend plugin manifests after a Tauri install", async () => {
    const {
      backend,
      clearPreservedPluginEditorExtensions,
      createApi,
      getBackend,
      installSource,
      pluginStore,
      service,
    } =
      await loadPluginInstallService();

    await service.installLocalPlugin(new ArrayBuffer(4), "Spoiler");

    expect(getBackend).toHaveBeenCalledTimes(1);
    expect(backend.installPlugin).toHaveBeenCalledTimes(1);
    expect(backend.installPlugin.mock.calls[0]?.[0]).toBeInstanceOf(Uint8Array);
    expect(backend.installPlugin.mock.calls[0]?.[0].byteLength).toBe(4);
    expect(clearPreservedPluginEditorExtensions).toHaveBeenCalledWith(
      "diaryx.spoiler",
    );
    expect(createApi).toHaveBeenCalledWith(backend);
    expect(pluginStore.init).toHaveBeenCalledWith({ kind: "mock-api" });
    expect(pluginStore.preloadInsertCommandIcons).toHaveBeenCalledTimes(1);
    expect(installSource.getInstalledPluginSource("diaryx.spoiler")).toBe("local");
  });

  it("clears enabled state and refreshes manifests after a Tauri uninstall", async () => {
    const { backend, createApi, installSource, pluginStore, preservePluginEditorExtensions, service } =
      await loadPluginInstallService();

    installSource.setInstalledPluginSource("diaryx.spoiler", "local");

    await service.uninstallPlugin("diaryx.spoiler");

    expect(backend.uninstallPlugin).toHaveBeenCalledWith("diaryx.spoiler");
    expect(pluginStore.clearPluginEnabled).toHaveBeenCalledWith(
      "diaryx.spoiler",
    );
    expect(preservePluginEditorExtensions).toHaveBeenCalledWith(
      pluginStore.allManifests[0],
    );
    expect(createApi).toHaveBeenCalledWith(backend);
    expect(pluginStore.init).toHaveBeenCalledWith({ kind: "mock-api" });
    expect(pluginStore.preloadInsertCommandIcons).toHaveBeenCalledTimes(1);
    expect(installSource.getInstalledPluginSource("diaryx.spoiler")).toBeNull();
  });
});
