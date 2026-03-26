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

function makePluginManifest(overrides?: Record<string, unknown>) {
  return {
    id: "diaryx.sync",
    name: "Sync",
    version: "1.0.0",
    description: "Sync plugin",
    capabilities: [],
    ui: [],
    cli: [],
    ...overrides,
  };
}

function makePluginInstance(manifest: ReturnType<typeof makePluginManifest>) {
  return {
    manifest,
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
}

async function loadBrowserPluginManager(opts?: {
  runtimeSupported?: boolean;
  runtimeReason?: string;
  inspectResult?: unknown;
  pluginManifestOverrides?: Record<string, unknown>;
  loadBrowserPluginOverride?: unknown;
}) {
  vi.resetModules();
  stubIndexedDbEmpty();

  const pluginManifest = makePluginManifest(opts?.pluginManifestOverrides);
  const pluginInstance = makePluginInstance(pluginManifest);

  const extismMocks = {
    inspectBrowserPlugin: vi.fn().mockResolvedValue(
      opts?.inspectResult ?? {
        manifest: pluginManifest,
        requestedPermissions: {
          defaults: {
            read_files: { include: ["all"], exclude: [] },
            http_requests: { include: ["all"], exclude: [] },
          },
          reasons: {},
        },
      },
    ),
    loadBrowserPlugin:
      opts?.loadBrowserPluginOverride !== undefined
        ? vi.fn(opts.loadBrowserPluginOverride as () => unknown)
        : vi.fn().mockResolvedValue(pluginInstance),
    getBrowserPluginRuntimeSupport: vi.fn(() => ({
      supported: opts?.runtimeSupported ?? true,
      reason: opts?.runtimeReason,
    })),
  };

  const assetMocks = {
    deleteWorkspaceTree: vi.fn().mockResolvedValue(undefined),
    getPluginInstallPath: vi
      .fn()
      .mockImplementation(
        (pluginId: string) => `.diaryx/plugins/${pluginId}/plugin.wasm`,
      ),
    listWorkspaceFiles: vi.fn().mockResolvedValue([]),
    readWorkspaceBinary: vi.fn().mockResolvedValue(null),
    writeWorkspaceBinary: vi.fn().mockResolvedValue(undefined),
  };

  const pluginStore = {
    clearPluginEnabled: vi.fn(),
    isPluginEnabled: vi.fn(() => true),
  };

  const editorExtensionMocks = {
    createExtensionFromManifest: vi.fn(),
    createMarkFromManifest: vi.fn(),
    getBuiltinExtension: vi.fn(() => null),
    isEditorExtension: vi.fn(() => false),
  };

  const preservedEditorMocks = {
    clearPreservedPluginEditorExtensions: vi.fn(),
    preservePluginEditorExtensions: vi.fn(),
  };

  const transcoderMocks = {
    registerTranscoder: vi.fn(),
    unregisterTranscodersByPlugin: vi.fn(),
    clearAllTranscoders: vi.fn(),
  };

  vi.doMock("./extismBrowserLoader", () => extismMocks);
  vi.doMock("./editorExtensionFactory", () => editorExtensionMocks);
  vi.doMock("./preservedEditorExtensions.svelte", () => preservedEditorMocks);
  vi.doMock("$lib/workspace/workspaceAssetStorage", () => assetMocks);
  vi.doMock("@/models/stores/pluginStore.svelte", () => ({
    getPluginStore: () => pluginStore,
  }));
  vi.doMock("@/models/services/imageConverterService", () => transcoderMocks);

  const module = await import("./browserPluginManager.svelte");

  return {
    assetMocks,
    editorExtensionMocks,
    extismMocks,
    module,
    pluginInstance,
    pluginManifest,
    pluginStore,
    preservedEditorMocks,
    transcoderMocks,
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
    const { assetMocks, extismMocks, module } =
      await loadBrowserPluginManager();
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
    const { assetMocks, extismMocks, module } =
      await loadBrowserPluginManager();
    const persistor = vi.fn().mockResolvedValue(undefined);

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
    ]);
    assetMocks.readWorkspaceBinary.mockResolvedValue(
      new Uint8Array([1, 2, 3]),
    );

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

describe("installPlugin", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("throws when runtime is unsupported", async () => {
    const { module } = await loadBrowserPluginManager({
      runtimeSupported: false,
      runtimeReason: "No WASM support",
    });

    await expect(
      module.installPlugin(new ArrayBuffer(4), "Sync"),
    ).rejects.toThrow("No WASM support");

    expect(module.getBrowserPluginSupportError()).toBe("No WASM support");
  });

  it("throws generic message when runtime unsupported without reason", async () => {
    const { module } = await loadBrowserPluginManager({
      runtimeSupported: false,
    });

    await expect(
      module.installPlugin(new ArrayBuffer(4), "Sync"),
    ).rejects.toThrow("Browser plugins are not supported in this runtime.");
  });

  it("writes wasm bytes to workspace storage", async () => {
    const { module, assetMocks } = await loadBrowserPluginManager();
    const wasmBytes = new ArrayBuffer(8);

    await module.installPlugin(wasmBytes, "Sync");

    expect(assetMocks.writeWorkspaceBinary).toHaveBeenCalledWith(
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
      expect.any(Uint8Array),
    );
  });

  it("returns the installed plugin manifest", async () => {
    const { module, pluginManifest } = await loadBrowserPluginManager();

    const result = await module.installPlugin(new ArrayBuffer(4), "Sync");

    expect(result).toBe(pluginManifest);
  });

  it("adds manifest to browser manifests list", async () => {
    const { module } = await loadBrowserPluginManager();

    expect(module.getBrowserManifests()).toHaveLength(0);
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(module.getBrowserManifests()).toHaveLength(1);
    expect(module.getBrowserManifests()[0].id).toBe("diaryx.sync");
  });

  it("makes plugin available via getPlugin", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    expect(module.getPlugin("diaryx.sync")).toBeUndefined();
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(module.getPlugin("diaryx.sync")).toBe(pluginInstance);
  });

  it("closes and replaces existing plugin on reinstall", async () => {
    const manifest = makePluginManifest();
    const firstInstance = makePluginInstance(manifest);
    const secondInstance = makePluginInstance(manifest);
    let callCount = 0;

    const { module } = await loadBrowserPluginManager({
      loadBrowserPluginOverride: () => {
        callCount++;
        return Promise.resolve(callCount === 1 ? firstInstance : secondInstance);
      },
    });

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(module.getPlugin("diaryx.sync")).toBe(firstInstance);

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(firstInstance.close).toHaveBeenCalledTimes(1);
    expect(module.getPlugin("diaryx.sync")).toBe(secondInstance);
    expect(module.getBrowserManifests()).toHaveLength(1);
  });

  it("skips persisting defaults when no persistor is configured", async () => {
    const { module, extismMocks } = await loadBrowserPluginManager();
    // Don't set persistor — should still install without error
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(extismMocks.loadBrowserPlugin).toHaveBeenCalledTimes(1);
  });

  it("skips persisting defaults when requested permissions have no defaults", async () => {
    const { module } = await loadBrowserPluginManager({
      inspectResult: {
        manifest: makePluginManifest(),
        requestedPermissions: { defaults: undefined, reasons: {} },
      },
    });

    const persistor = vi.fn().mockResolvedValue(undefined);
    module.setPluginPermissionConfigProvider(() => ({}));
    module.setPluginPermissionConfigPersistor(persistor);

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(persistor).not.toHaveBeenCalled();
  });

  it("clears preserved editor extensions on install", async () => {
    const { module, preservedEditorMocks } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");

    expect(
      preservedEditorMocks.clearPreservedPluginEditorExtensions,
    ).toHaveBeenCalledWith("diaryx.sync");
  });

  it("invalidates editor extensions after install", async () => {
    const { module } = await loadBrowserPluginManager();

    const versionBefore = module.getPluginExtensionsVersion();
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(module.getPluginExtensionsVersion()).toBeGreaterThan(versionBefore);
  });
});

describe("uninstallPlugin", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("closes plugin, deletes storage, clears enabled flag, and removes manifest", async () => {
    const { module, pluginInstance, assetMocks, pluginStore } =
      await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(module.getBrowserManifests()).toHaveLength(1);

    await module.uninstallPlugin("diaryx.sync");

    expect(pluginInstance.close).toHaveBeenCalledTimes(1);
    expect(assetMocks.deleteWorkspaceTree).toHaveBeenCalledWith(
      ".diaryx/plugins/diaryx.sync",
    );
    expect(pluginStore.clearPluginEnabled).toHaveBeenCalledWith("diaryx.sync");
    expect(module.getBrowserManifests()).toHaveLength(0);
    expect(module.getPlugin("diaryx.sync")).toBeUndefined();
  });

  it("handles uninstalling a plugin that is not loaded", async () => {
    const { module, assetMocks, pluginStore } =
      await loadBrowserPluginManager();

    // Should not throw
    await module.uninstallPlugin("nonexistent.plugin");

    expect(assetMocks.deleteWorkspaceTree).toHaveBeenCalledWith(
      ".diaryx/plugins/nonexistent.plugin",
    );
    expect(pluginStore.clearPluginEnabled).toHaveBeenCalledWith(
      "nonexistent.plugin",
    );
  });

  it("preserves plugin editor extensions on uninstall", async () => {
    const { module, preservedEditorMocks, pluginManifest } =
      await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.uninstallPlugin("diaryx.sync");

    expect(
      preservedEditorMocks.preservePluginEditorExtensions,
    ).toHaveBeenCalledWith(pluginManifest);
  });

  it("unregisters transcoders on uninstall", async () => {
    const { module, transcoderMocks } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.uninstallPlugin("diaryx.sync");

    expect(transcoderMocks.unregisterTranscodersByPlugin).toHaveBeenCalledWith(
      "diaryx.sync",
    );
  });

  it("invalidates editor extensions after uninstall", async () => {
    const { module } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    const versionAfterInstall = module.getPluginExtensionsVersion();
    await module.uninstallPlugin("diaryx.sync");
    expect(module.getPluginExtensionsVersion()).toBeGreaterThan(
      versionAfterInstall,
    );
  });
});

describe("loadAllPlugins", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("returns early when runtime is unsupported", async () => {
    const { module, assetMocks } = await loadBrowserPluginManager({
      runtimeSupported: false,
      runtimeReason: "No WASM",
    });

    await module.loadAllPlugins();

    expect(assetMocks.listWorkspaceFiles).not.toHaveBeenCalled();
    expect(module.getBrowserPluginSupportError()).toBe("No WASM");
  });

  it("loads plugins from workspace storage", async () => {
    const { module, assetMocks, extismMocks } =
      await loadBrowserPluginManager();

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
    ]);
    assetMocks.readWorkspaceBinary.mockResolvedValue(
      new Uint8Array([1, 2, 3]),
    );

    await module.loadAllPlugins();

    expect(extismMocks.inspectBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(extismMocks.loadBrowserPlugin).toHaveBeenCalledTimes(1);
    expect(module.getBrowserManifests()).toHaveLength(1);
    expect(module.getPlugin("diaryx.sync")).toBeDefined();
  });

  it("skips files that are not plugin.wasm", async () => {
    const { module, assetMocks, extismMocks } =
      await loadBrowserPluginManager();

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/diaryx.sync/config.json",
      ".diaryx/plugins/diaryx.sync/README.md",
    ]);

    await module.loadAllPlugins();

    expect(extismMocks.loadBrowserPlugin).not.toHaveBeenCalled();
    expect(module.getBrowserManifests()).toHaveLength(0);
  });

  it("skips plugins whose wasm bytes cannot be read", async () => {
    const { module, assetMocks, extismMocks } =
      await loadBrowserPluginManager();

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/diaryx.sync/plugin.wasm",
    ]);
    assetMocks.readWorkspaceBinary.mockResolvedValue(null);

    await module.loadAllPlugins();

    expect(extismMocks.loadBrowserPlugin).not.toHaveBeenCalled();
    expect(module.getBrowserManifests()).toHaveLength(0);
  });

  it("continues loading remaining plugins when one fails", async () => {
    const manifestB = makePluginManifest({ id: "plugin.b", name: "Plugin B" });
    const instanceB = makePluginInstance(manifestB);

    let inspectCallCount = 0;

    const { module, assetMocks, extismMocks } =
      await loadBrowserPluginManager();

    assetMocks.listWorkspaceFiles.mockResolvedValue([
      ".diaryx/plugins/plugin.a/plugin.wasm",
      ".diaryx/plugins/plugin.b/plugin.wasm",
    ]);
    assetMocks.readWorkspaceBinary.mockResolvedValue(
      new Uint8Array([1, 2, 3]),
    );

    extismMocks.inspectBrowserPlugin.mockImplementation(() => {
      inspectCallCount++;
      if (inspectCallCount === 1) {
        return Promise.reject(new Error("Corrupt WASM"));
      }
      return Promise.resolve({
        manifest: manifestB,
        requestedPermissions: undefined,
      });
    });
    extismMocks.loadBrowserPlugin.mockResolvedValue(instanceB);

    await module.loadAllPlugins();

    // First plugin failed, second should still load
    expect(module.getBrowserManifests()).toHaveLength(1);
    expect(module.getBrowserManifests()[0].id).toBe("plugin.b");
  });

  it("clears runtime support error on successful load", async () => {
    // First, trigger an error
    const { module: module1 } = await loadBrowserPluginManager({
      runtimeSupported: false,
      runtimeReason: "No WASM",
    });
    await module1.loadAllPlugins();
    expect(module1.getBrowserPluginSupportError()).toBe("No WASM");

    // Now load again with support
    const { module: module2 } = await loadBrowserPluginManager({
      runtimeSupported: true,
    });
    await module2.loadAllPlugins();
    expect(module2.getBrowserPluginSupportError()).toBeNull();
  });

  it("invalidates editor extensions after loading all plugins", async () => {
    const { module } = await loadBrowserPluginManager();

    const versionBefore = module.getPluginExtensionsVersion();
    await module.loadAllPlugins();
    expect(module.getPluginExtensionsVersion()).toBeGreaterThan(versionBefore);
  });
});

describe("inspectPluginWasm", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("returns pluginId, pluginName, and requestedPermissions", async () => {
    const { module } = await loadBrowserPluginManager();

    const result = await module.inspectPluginWasm(new ArrayBuffer(4));

    expect(result.pluginId).toBe("diaryx.sync");
    expect(result.pluginName).toBe("Sync");
    expect(result.requestedPermissions).toBeDefined();
    expect(result.requestedPermissions?.defaults).toEqual({
      read_files: { include: ["all"], exclude: [] },
      http_requests: { include: ["all"], exclude: [] },
    });
  });

  it("falls back to id for name when manifest has no name", async () => {
    const { module } = await loadBrowserPluginManager({
      inspectResult: {
        manifest: { id: "diaryx.unnamed", version: "1.0.0" },
        requestedPermissions: undefined,
      },
    });

    const result = await module.inspectPluginWasm(new ArrayBuffer(4));

    expect(result.pluginId).toBe("diaryx.unnamed");
    expect(result.pluginName).toBe("diaryx.unnamed");
  });
});

describe("getPlugin / getBrowserManifests", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("getPlugin returns undefined for unknown id", async () => {
    const { module } = await loadBrowserPluginManager();
    expect(module.getPlugin("nonexistent")).toBeUndefined();
  });

  it("getBrowserManifests returns empty array initially", async () => {
    const { module } = await loadBrowserPluginManager();
    expect(module.getBrowserManifests()).toEqual([]);
  });

  it("getBrowserManifests returns manifests after install", async () => {
    const { module } = await loadBrowserPluginManager();
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    const manifests = module.getBrowserManifests();
    expect(manifests).toHaveLength(1);
    expect(manifests[0].name).toBe("Sync");
  });
});

describe("plugin permission config provider/persistor", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("setPluginPermissionConfigProvider sets provider used during install", async () => {
    const { module, extismMocks } = await loadBrowserPluginManager();

    const config = { "diaryx.sync": { permissions: {} } };
    module.setPluginPermissionConfigProvider(() => config);

    await module.installPlugin(new ArrayBuffer(4), "Sync");

    // Verify loadBrowserPlugin was called with options that include getPluginsConfig
    const callOpts = extismMocks.loadBrowserPlugin.mock.calls[0][1];
    expect(callOpts.getPluginsConfig()).toBe(config);
  });

  it("setPluginPermissionConfigProvider(null) clears provider", async () => {
    const { module, extismMocks } = await loadBrowserPluginManager();

    module.setPluginPermissionConfigProvider(() => ({ test: { permissions: {} } }));
    module.setPluginPermissionConfigProvider(null);

    await module.installPlugin(new ArrayBuffer(4), "Sync");

    const callOpts = extismMocks.loadBrowserPlugin.mock.calls[0][1];
    expect(callOpts.getPluginsConfig()).toBeUndefined();
  });

  it("setPluginPermissionConfigPersistor(null) disables persistence", async () => {
    const { module } = await loadBrowserPluginManager();

    const persistor = vi.fn().mockResolvedValue(undefined);
    module.setPluginPermissionConfigPersistor(persistor);
    module.setPluginPermissionConfigPersistor(null);

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    expect(persistor).not.toHaveBeenCalled();
  });
});

describe("getBrowserPluginSupport", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("returns runtime support status", async () => {
    const { module } = await loadBrowserPluginManager({
      runtimeSupported: true,
    });
    expect(module.getBrowserPluginSupport()).toEqual({ supported: true });
  });

  it("returns reason when unsupported", async () => {
    const { module } = await loadBrowserPluginManager({
      runtimeSupported: false,
      runtimeReason: "Missing feature",
    });
    expect(module.getBrowserPluginSupport()).toEqual({
      supported: false,
      reason: "Missing feature",
    });
  });
});

describe("editor extension invalidation", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("getPluginExtensionsVersion increments on install and uninstall", async () => {
    const { module } = await loadBrowserPluginManager();

    const v0 = module.getPluginExtensionsVersion();
    await module.installPlugin(new ArrayBuffer(4), "Sync");
    const v1 = module.getPluginExtensionsVersion();
    expect(v1).toBeGreaterThan(v0);

    await module.uninstallPlugin("diaryx.sync");
    const v2 = module.getPluginExtensionsVersion();
    expect(v2).toBeGreaterThan(v1);
  });

  it("getEditorExtensions returns empty array when no plugins are loaded", async () => {
    const { module } = await loadBrowserPluginManager();
    expect(module.getEditorExtensions()).toEqual([]);
  });
});

describe("event dispatch", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("normalizes backslash and redundant slash paths", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.dispatchFileOpenedEvent("workspace\\\\subdir//file.md");

    expect(pluginInstance.callEvent).toHaveBeenCalledWith({
      event_type: "file_opened",
      payload: { path: "workspace/subdir/file.md" },
    });
  });

  it("strips leading ./ from paths", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.dispatchFileCreatedEvent("./new-file.md");

    expect(pluginInstance.callEvent).toHaveBeenCalledWith({
      event_type: "file_created",
      payload: { path: "new-file.md" },
    });
  });

  it("dispatches file_deleted event", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.dispatchFileDeletedEvent("old-file.md");

    expect(pluginInstance.callEvent).toHaveBeenCalledWith({
      event_type: "file_deleted",
      payload: { path: "old-file.md" },
    });
  });

  it("skips disabled plugins during event dispatch", async () => {
    const { module, pluginInstance, pluginStore } =
      await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    pluginStore.isPluginEnabled.mockReturnValue(false);

    await module.dispatchFileSavedEvent("test.md");

    expect(pluginInstance.callEvent).not.toHaveBeenCalled();
  });

  it("notifies event observers", async () => {
    const { module } = await loadBrowserPluginManager();

    const observer = vi.fn();
    const unsubscribe = module.onPluginEventDispatched(observer);

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    await module.dispatchFileSavedEvent("test.md");

    expect(observer).toHaveBeenCalledWith({
      event_type: "file_saved",
      payload: { path: "test.md", body_changed: true },
    });

    unsubscribe();
    await module.dispatchFileSavedEvent("test2.md");
    // Should not be called again after unsubscribe
    expect(observer).toHaveBeenCalledTimes(1);
  });
});

describe("dispatchCommand", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("dispatches command to loaded plugin", async () => {
    const { module, pluginInstance } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    const result = await module.dispatchCommand("diaryx.sync", "test_cmd", {
      key: "value",
    });

    expect(result).toEqual({ success: true });
    expect(pluginInstance.callCommand).toHaveBeenCalledWith(
      "test_cmd",
      { key: "value" },
      undefined,
    );
  });

  it("returns error for disabled plugin", async () => {
    const { module, pluginStore } = await loadBrowserPluginManager();

    await module.installPlugin(new ArrayBuffer(4), "Sync");
    pluginStore.isPluginEnabled.mockReturnValue(false);

    const result = await module.dispatchCommand(
      "diaryx.sync",
      "test_cmd",
      {},
    );

    expect(result.success).toBe(false);
    expect(result.error).toContain("disabled");
  });

  it("returns error for unloaded plugin", async () => {
    const { module } = await loadBrowserPluginManager();

    const result = await module.dispatchCommand(
      "nonexistent",
      "test_cmd",
      {},
    );

    expect(result.success).toBe(false);
    expect(result.error).toContain("not loaded");
  });
});
