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

async function loadBrowserPluginInstallService(options?: {
  inMemoryPluginsConfig?: Record<string, { permissions: Record<string, unknown> }>;
  frontmatterPlugins?: Record<string, { permissions: Record<string, unknown> }>;
}) {
  vi.resetModules();

  const inspectPluginWasm = vi.fn().mockResolvedValue({
    pluginId: "diaryx.sync",
    pluginName: "Sync",
    requestedPermissions: {
      defaults: {
        read_files: {
          include: ["all"],
          exclude: [],
        },
      },
    },
  });
  const installPlugin = vi.fn().mockResolvedValue({ id: "diaryx.sync" });
  const backend = { getWorkspacePath: vi.fn().mockReturnValue(".") };
  const api = {
    getFrontmatter: vi.fn().mockResolvedValue({
      plugins: options?.frontmatterPlugins ?? {},
    }),
    executePluginCommand: vi.fn().mockResolvedValue({ success: true }),
  };
  const createApi = vi.fn(() => api);
  const getBackend = vi.fn().mockResolvedValue(backend);
  const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

  vi.doMock("$lib/backend", () => ({
    getBackend,
    isTauri: () => false,
  }));
  vi.doMock("$lib/backend/api", () => ({
    createApi,
  }));
  vi.doMock("$lib/plugins/browserPluginManager.svelte", () => ({
    inspectPluginWasm,
    installPlugin,
    uninstallPlugin: vi.fn(),
  }));
  vi.doMock("$lib/plugins/preservedEditorExtensions.svelte", () => ({
    clearPreservedPluginEditorExtensions: vi.fn(),
    preservePluginEditorExtensions: vi.fn(),
  }));
  vi.doMock("@/models/stores/pluginStore.svelte", () => ({
    getPluginStore: () => ({
      allManifests: [],
      init: vi.fn().mockResolvedValue(undefined),
      preloadInsertCommandIcons: vi.fn().mockResolvedValue(undefined),
      clearPluginEnabled: vi.fn(),
    }),
  }));
  vi.doMock("@/models/stores/workspaceStore.svelte", () => ({
    workspaceStore: { tree: { path: "index.md" } },
  }));
  vi.doMock("@/models/stores/permissionStore.svelte", () => ({
    permissionStore: {
      getPluginsConfig: vi.fn(() => options?.inMemoryPluginsConfig),
    },
  }));
  vi.doMock("$lib/sync/browserWorkspaceMutationMirror", () => ({
    mirrorCurrentWorkspaceMutationToLinkedProviders: vi.fn().mockResolvedValue(undefined),
  }));

  const service = await import("./pluginInstallService");

  return {
    api,
    confirmSpy,
    createApi,
    getBackend,
    inspectPluginWasm,
    installPlugin,
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

  it("skips browser install confirm when workspace frontmatter already grants permissions", async () => {
    const { api, confirmSpy, createApi, getBackend, installPlugin, service } =
      await loadBrowserPluginInstallService({
        frontmatterPlugins: {
          "diaryx.sync": {
            permissions: {
              read_files: {
                include: ["all"],
                exclude: [],
              },
            },
          },
        },
      });

    await service.installLocalPlugin(new ArrayBuffer(4), "Sync");

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(getBackend).toHaveBeenCalled();
    expect(createApi).toHaveBeenCalled();
    expect(api.getFrontmatter).toHaveBeenCalledWith("index.md");
    expect(installPlugin).toHaveBeenCalledTimes(1);
  });
});
