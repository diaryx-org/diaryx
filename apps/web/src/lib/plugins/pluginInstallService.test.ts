import { beforeEach, describe, expect, it, vi } from "vitest";

type MockBackend = {
  installPlugin: ReturnType<typeof vi.fn>;
  inspectPlugin: ReturnType<typeof vi.fn>;
  uninstallPlugin: ReturnType<typeof vi.fn>;
  getWorkspacePath: ReturnType<typeof vi.fn>;
};

type MockPluginStore = {
  allManifests: Array<{ id: string; name?: string; ui: unknown[] }>;
  init: ReturnType<typeof vi.fn>;
  preloadInsertCommandIcons: ReturnType<typeof vi.fn>;
  clearPluginEnabled: ReturnType<typeof vi.fn>;
};

async function loadPluginInstallService(options?: {
  backendWorkspacePath?: string;
  frontmatterPlugins?: Record<string, { permissions: Record<string, unknown> }>;
  resolvedRootIndexPath?: string;
  workspaceTreePath?: string | null;
  currentWorkspaceId?: string | null;
  pluginMetadata?: Record<string, Record<string, unknown>>;
}) {
  vi.resetModules();

  const backend: MockBackend = {
    installPlugin: vi
      .fn()
      .mockResolvedValue(
        JSON.stringify({ id: "diaryx.spoiler", name: "Spoiler" }),
      ),
    inspectPlugin: vi.fn().mockResolvedValue({
      pluginId: "diaryx.spoiler",
      pluginName: "Spoiler",
      requestedPermissions: {
        defaults: {
          read_files: {
            include: ["all"],
            exclude: [],
          },
        },
      },
    }),
    uninstallPlugin: vi.fn().mockResolvedValue(undefined),
    getWorkspacePath: vi
      .fn()
      .mockReturnValue(options?.backendWorkspacePath ?? "workspace/index.md"),
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
  const api = {
    kind: "mock-api",
    resolveWorkspaceRootIndexPath: vi
      .fn()
      .mockResolvedValue(
        options?.resolvedRootIndexPath ?? options?.backendWorkspacePath ?? "workspace/index.md",
      ),
    getFrontmatter: vi.fn().mockResolvedValue({
      plugins: options?.frontmatterPlugins ?? {},
    }),
    setFrontmatterProperty: vi.fn().mockResolvedValue(null),
    removeFrontmatterProperty: vi.fn().mockResolvedValue(undefined),
    removeWorkspacePluginData: vi.fn().mockResolvedValue(undefined),
  };
  const createApi = vi.fn(() => api);
  const getBackend = vi.fn().mockResolvedValue(backend);
  const clearPreservedPluginEditorExtensions = vi.fn();
  const preservePluginEditorExtensions = vi.fn();
  const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
  const proxyFetch = vi
    .fn()
    .mockResolvedValue(new Response(new TextEncoder().encode("test"), { status: 200 }));

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
  vi.doMock("$lib/backend/proxyFetch", () => ({
    proxyFetch,
  }));
  vi.doMock("@/models/stores/pluginStore.svelte", () => ({
    getPluginStore: () => pluginStore,
  }));
  vi.doMock("@/models/stores/workspaceStore.svelte", () => ({
    workspaceStore: {
      tree:
        options?.workspaceTreePath === undefined
          ? null
          : options.workspaceTreePath === null
            ? null
            : { path: options.workspaceTreePath },
    },
  }));
  vi.doMock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
    getCurrentWorkspaceId: vi.fn(() => options?.currentWorkspaceId ?? "local-1"),
    getPluginMetadata: vi.fn((workspaceId: string, pluginId: string) =>
      workspaceId === (options?.currentWorkspaceId ?? "local-1")
        ? options?.pluginMetadata?.[pluginId]
        : undefined,
    ),
    setPluginMetadata: vi.fn(),
  }));
  vi.doMock("$lib/namespace/namespaceService", () => ({
    deleteNamespace: vi.fn().mockResolvedValue(undefined),
  }));
  vi.doMock("@/models/stores/permissionStore.svelte", () => ({
    permissionStore: {
      getPluginsConfig: vi.fn(() => undefined),
    },
  }));
  vi.doMock("$lib/sync/browserWorkspaceMutationMirror", () => ({
    mirrorCurrentWorkspaceMutationToLinkedProviders: vi.fn().mockResolvedValue(undefined),
  }));

  const service = await import("./pluginInstallService");
  const installSource = await import("./pluginInstallSource.svelte");
  const workspaceRegistry = await import("$lib/storage/localWorkspaceRegistry.svelte");
  const namespaceService = await import("$lib/namespace/namespaceService");

  return {
    backend,
    clearPreservedPluginEditorExtensions,
    confirmSpy,
    createApi,
    getBackend,
    installSource,
    api,
    pluginStore,
    proxyFetch,
    preservePluginEditorExtensions,
    namespaceService,
    service,
    workspaceRegistry,
  };
}

async function loadBrowserPluginInstallService(options?: {
  inMemoryPluginsConfig?: Record<string, { permissions: Record<string, unknown> }>;
  frontmatterPlugins?: Record<string, { permissions: Record<string, unknown> }>;
  currentWorkspaceId?: string | null;
  pluginMetadata?: Record<string, Record<string, unknown>>;
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
    resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("index.md"),
    getFrontmatter: vi.fn().mockResolvedValue({
      plugins: options?.frontmatterPlugins ?? {},
    }),
    removeFrontmatterProperty: vi.fn().mockResolvedValue(undefined),
    removeWorkspacePluginData: vi.fn().mockResolvedValue(undefined),
    executePluginCommand: vi.fn().mockResolvedValue({ success: true }),
  };
  const createApi = vi.fn(() => api);
  const getBackend = vi.fn().mockResolvedValue(backend);
  const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
  const proxyFetch = vi.fn();

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
  vi.doMock("$lib/backend/proxyFetch", () => ({
    proxyFetch,
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
  vi.doMock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
    getCurrentWorkspaceId: vi.fn(() => options?.currentWorkspaceId ?? "local-1"),
    getPluginMetadata: vi.fn((workspaceId: string, pluginId: string) =>
      workspaceId === (options?.currentWorkspaceId ?? "local-1")
        ? options?.pluginMetadata?.[pluginId]
        : undefined,
    ),
    setPluginMetadata: vi.fn(),
  }));
  vi.doMock("$lib/namespace/namespaceService", () => ({
    deleteNamespace: vi.fn().mockResolvedValue(undefined),
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
  const workspaceRegistry = await import("$lib/storage/localWorkspaceRegistry.svelte");
  const namespaceService = await import("$lib/namespace/namespaceService");

  return {
    api,
    confirmSpy,
    createApi,
    getBackend,
    inspectPluginWasm,
    installPlugin,
    namespaceService,
    proxyFetch,
    service,
    workspaceRegistry,
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

    expect(getBackend).toHaveBeenCalled();
    expect(backend.inspectPlugin).toHaveBeenCalledTimes(1);
    expect(backend.installPlugin).toHaveBeenCalledTimes(1);
    expect(backend.installPlugin.mock.calls[0]?.[0]).toBeInstanceOf(Uint8Array);
    expect(backend.installPlugin.mock.calls[0]?.[0].byteLength).toBe(4);
    expect(clearPreservedPluginEditorExtensions).toHaveBeenCalledWith(
      "diaryx.spoiler",
    );
    expect(createApi).toHaveBeenCalledWith(backend);
    expect(pluginStore.init).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "mock-api" }),
    );
    expect(pluginStore.preloadInsertCommandIcons).toHaveBeenCalledTimes(1);
    expect(installSource.getInstalledPluginSource("diaryx.spoiler")).toBe("local");
  });

  it("clears enabled state and refreshes manifests after a Tauri uninstall", async () => {
    const {
      api,
      backend,
      createApi,
      installSource,
      namespaceService,
      pluginStore,
      preservePluginEditorExtensions,
      service,
      workspaceRegistry,
    } = await loadPluginInstallService({
      frontmatterPlugins: {
        "diaryx.spoiler": { permissions: {} },
        "diaryx.other": { permissions: {} },
      },
      pluginMetadata: {
        "diaryx.spoiler": {
          namespace_id: "ns-123",
        },
      },
    });

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
    expect(pluginStore.init).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "mock-api" }),
    );
    expect(pluginStore.preloadInsertCommandIcons).toHaveBeenCalledTimes(1);
    expect(api.removeWorkspacePluginData).toHaveBeenCalledWith(
      "workspace/index.md",
      "diaryx.spoiler",
    );
    expect(namespaceService.deleteNamespace).toHaveBeenCalledWith("ns-123");
    expect(workspaceRegistry.setPluginMetadata).toHaveBeenCalledWith(
      "local-1",
      "diaryx.spoiler",
      null,
    );
    expect(installSource.getInstalledPluginSource("diaryx.spoiler")).toBeNull();
  });

  it("removes workspace plugin data through the backend helper on browser uninstall", async () => {
    const { api, namespaceService, service, workspaceRegistry } = await loadBrowserPluginInstallService({
      pluginMetadata: {
        "diaryx.spoiler": {
          remoteWorkspaceId: "remote-1",
        },
      },
    });

    await service.uninstallPlugin("diaryx.spoiler");

    expect(api.removeWorkspacePluginData).toHaveBeenCalledWith(
      "index.md",
      "diaryx.spoiler",
    );
    expect(namespaceService.deleteNamespace).toHaveBeenCalledWith("remote-1");
    expect(workspaceRegistry.setPluginMetadata).toHaveBeenCalledWith(
      "local-1",
      "diaryx.spoiler",
      null,
    );
  });

  it("reviews requested permissions before a Tauri install", async () => {
    const { backend, confirmSpy, service } = await loadPluginInstallService();

    await service.installLocalPlugin(new ArrayBuffer(4), "Spoiler");

    expect(confirmSpy).toHaveBeenCalledTimes(1);
    expect(backend.inspectPlugin).toHaveBeenCalledTimes(1);
    expect(backend.installPlugin).toHaveBeenCalledTimes(1);
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

  it("resolves a directory workspace root before reading or writing plugin permissions", async () => {
    const workspaceDir = "/Users/test/Documents/ppppppp/";
    const rootIndexPath = "/Users/test/Documents/ppppppp/README.md";
    const { api, service } = await loadPluginInstallService({
      backendWorkspacePath: workspaceDir,
      resolvedRootIndexPath: rootIndexPath,
      workspaceTreePath: workspaceDir,
    });

    await service.installLocalPlugin(new ArrayBuffer(4), "Spoiler");

    expect(api.resolveWorkspaceRootIndexPath).toHaveBeenCalledWith(workspaceDir);
    expect(api.getFrontmatter.mock.calls.every(([path]) => path === rootIndexPath)).toBe(true);
    expect(api.setFrontmatterProperty).toHaveBeenCalledWith(
      rootIndexPath,
      "plugins",
      expect.any(Object),
      rootIndexPath,
    );
  });

  it("downloads registry plugin bytes through proxyFetch on Tauri", async () => {
    const { backend, proxyFetch, service } = await loadPluginInstallService();

    await service.installRegistryPlugin({
      id: "diaryx.spoiler",
      name: "Spoiler",
      version: "1.0.0",
      summary: "Spoilers",
      description: "Spoilers",
      author: "Diaryx",
      license: "MIT",
      repository: null,
      categories: [],
      tags: [],
      icon: null,
      screenshots: [],
      capabilities: [],
      requested_permissions: null,
      artifact: {
        url: "https://app.diaryx.org/cdn/plugins/artifacts/diaryx.spoiler/1.0.0/plugin.wasm",
        sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        size: 4,
        published_at: "2026-03-14T00:00:00Z",
      },
    });

    expect(proxyFetch).toHaveBeenCalledWith(
      "https://app.diaryx.org/cdn/plugins/artifacts/diaryx.spoiler/1.0.0/plugin.wasm",
    );
    expect(backend.installPlugin).toHaveBeenCalledTimes(1);
  });

  it("reports the failing Tauri install stage for registry plugins", async () => {
    const { backend, service } = await loadPluginInstallService();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    backend.installPlugin.mockRejectedValue({
      message: "Failed to write plugin WASM into workspace: Operation not permitted",
      kind: "IoError",
      path: "/workspace/.diaryx/plugins/diaryx.spoiler/plugin.wasm",
    });

    await expect(
      service.installRegistryPlugin({
        id: "diaryx.spoiler",
        name: "Spoiler",
        version: "1.0.0",
        summary: "Spoilers",
        description: "Spoilers",
        author: "Diaryx",
        license: "MIT",
        repository: null,
        categories: [],
        tags: [],
        icon: null,
        screenshots: [],
        capabilities: [],
        requested_permissions: null,
        artifact: {
          url: "https://app.diaryx.org/cdn/plugins/artifacts/diaryx.spoiler/1.0.0/plugin.wasm",
          sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
          size: 4,
          published_at: "2026-03-14T00:00:00Z",
        },
      }),
    ).rejects.toThrow(
      "install plugin through Tauri backend failed: Failed to write plugin WASM into workspace: Operation not permitted (IoError: /workspace/.diaryx/plugins/diaryx.spoiler/plugin.wasm)",
    );

    expect(consoleError).toHaveBeenCalledWith(
      "[pluginInstallService] install plugin through Tauri backend failed",
      expect.objectContaining({
        expectedPluginId: "diaryx.spoiler",
        fallbackName: "Spoiler",
        bytes: 4,
        formattedError:
          "Failed to write plugin WASM into workspace: Operation not permitted (IoError: /workspace/.diaryx/plugins/diaryx.spoiler/plugin.wasm)",
      }),
    );
  });
});
