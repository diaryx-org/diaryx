import { beforeEach, describe, expect, it, vi } from "vitest";

function stubIndexedDbEmpty(): void {
  const db = {
    objectStoreNames: { contains: () => true },
    transaction: () => ({
      objectStore: () => ({
        getAll: () => {
          const request: {
            result?: unknown[];
            onsuccess?: () => void;
            onerror?: () => void;
          } = {};
          queueMicrotask(() => {
            request.result = [];
            request.onsuccess?.();
          });
          return request;
        },
      }),
    }),
  };

  vi.stubGlobal("indexedDB", {
    open: vi.fn(() => {
      const request: {
        result?: typeof db;
        onupgradeneeded?: () => void;
        onsuccess?: () => void;
        onerror?: () => void;
      } = {};
      queueMicrotask(() => {
        request.result = db;
        request.onsuccess?.();
      });
      return request;
    }),
  });
}

async function loadBrowserPluginManager() {
  vi.resetModules();
  stubIndexedDbEmpty();

  const pluginManifest = {
    id: "diaryx.sync",
    name: "Sync",
    version: "1.0.0",
    description: "Sync plugin",
    capabilities: [],
    ui: [],
    cli: [],
  };
  const pluginInstance = {
    manifest: pluginManifest,
    callLifecycle: vi.fn().mockResolvedValue(undefined),
    callEvent: vi.fn().mockResolvedValue(undefined),
    callCommand: vi.fn().mockResolvedValue({ success: true }),
    callTypedCommand: vi.fn().mockResolvedValue(null),
    callBinary: vi.fn().mockResolvedValue(null),
    getConfig: vi.fn().mockResolvedValue({}),
    setConfig: vi.fn().mockResolvedValue(undefined),
    callRender: vi.fn().mockResolvedValue({}),
    close: vi.fn().mockResolvedValue(undefined),
  };

  const extismMocks = {
    inspectBrowserPlugin: vi.fn().mockResolvedValue({
      manifest: pluginManifest,
      requestedPermissions: {
        defaults: {
          read_files: { include: ["all"], exclude: [] },
          http_requests: { include: ["all"], exclude: [] },
        },
        reasons: {},
      },
    }),
    loadBrowserPlugin: vi.fn().mockResolvedValue(pluginInstance),
    getBrowserPluginRuntimeSupport: vi.fn(() => ({ supported: true })),
  };

  const assetMocks = {
    deleteWorkspaceTree: vi.fn().mockResolvedValue(undefined),
    getPluginInstallPath: vi
      .fn()
      .mockImplementation((pluginId: string) => `.diaryx/plugins/${pluginId}/plugin.wasm`),
    listWorkspaceFiles: vi.fn().mockResolvedValue([]),
    readWorkspaceBinary: vi.fn().mockResolvedValue(null),
    writeWorkspaceBinary: vi.fn().mockResolvedValue(undefined),
  };

  const pluginStore = {
    clearPluginEnabled: vi.fn(),
    isPluginEnabled: vi.fn(() => true),
  };

  vi.doMock("./extismBrowserLoader", () => extismMocks);
  vi.doMock("./editorExtensionFactory", () => ({
    createExtensionFromManifest: vi.fn(),
    createMarkFromManifest: vi.fn(),
    getBuiltinExtension: vi.fn(() => null),
    isEditorExtension: vi.fn(() => false),
  }));
  vi.doMock("./preservedEditorExtensions.svelte", () => ({
    clearPreservedPluginEditorExtensions: vi.fn(),
    preservePluginEditorExtensions: vi.fn(),
  }));
  vi.doMock("$lib/workspace/workspaceAssetStorage", () => assetMocks);
  vi.doMock("@/models/stores/pluginStore.svelte", () => ({
    getPluginStore: () => pluginStore,
  }));

  const module = await import("./browserPluginManager.svelte");

  return {
    assetMocks,
    extismMocks,
    module,
    pluginInstance,
    pluginManifest,
  };
}

describe("browserPluginManager permission defaults", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("preserves absolute workspace paths in filesystem lifecycle events", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.dispatchFileSavedEvent("/workspace/live-propagation.md");
    await module.dispatchFileMovedEvent(
      "/workspace/old-name.md",
      "/workspace/new-name.md",
    );

    expect(pluginInstance.callEvent).toHaveBeenNthCalledWith(1, {
      event_type: "file_saved",
      payload: { path: "/workspace/live-propagation.md", body_changed: true },
    });
    expect(pluginInstance.callEvent).toHaveBeenNthCalledWith(2, {
      event_type: "file_moved",
      payload: {
        old_path: "/workspace/old-name.md",
        new_path: "/workspace/new-name.md",
      },
    });
  });

  it("persists requested defaults before installing a browser plugin", async () => {
    const { assetMocks, extismMocks, module } = await loadBrowserPluginManager();
    const persistor = vi.fn().mockResolvedValue(undefined);

    module.setPluginPermissionConfigProvider(() => ({}));
    module.setPluginPermissionConfigPersistor(persistor);

    await module.installPlugin(new ArrayBuffer(4), "Sync");

    expect(persistor).toHaveBeenCalledWith("diaryx.sync", {
      read_files: { include: ["all"], exclude: [] },
      http_requests: { include: ["all"], exclude: [] },
    });
    expect(extismMocks.inspectBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(extismMocks.loadBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(
      extismMocks.inspectBrowserPlugin.mock.invocationCallOrder[0],
    ).toBeLessThan(extismMocks.loadBrowserPlugin.mock.invocationCallOrder[0]);
    expect(assetMocks.writeWorkspaceBinary).toHaveBeenCalledWith(
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
      expect.any(Uint8Array),
    );
  });

  it("backfills requested defaults for stored browser plugins during load", async () => {
    const { assetMocks, extismMocks, module } = await loadBrowserPluginManager();
    const persistor = vi.fn().mockResolvedValue(undefined);

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
    ]);
    assetMocks.readWorkspaceBinary.mockResolvedValue(new Uint8Array([1, 2, 3]));

    module.setPluginPermissionConfigProvider(() => ({}));
    module.setPluginPermissionConfigPersistor(persistor);

    await module.loadAllPlugins();

    expect(extismMocks.inspectBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(extismMocks.loadBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(persistor).toHaveBeenCalledWith("diaryx.sync", {
      read_files: { include: ["all"], exclude: [] },
      http_requests: { include: ["all"], exclude: [] },
    });
  });
});
