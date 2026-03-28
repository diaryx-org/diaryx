import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  downloadWorkspace: vi.fn(),
  fetchPluginRegistry: vi.fn(),
  getBackend: vi.fn(),
  installRegistryPlugin: vi.fn(),
  loadAllPlugins: vi.fn(),
  removeLocalWorkspace: vi.fn(),
  resetBackend: vi.fn(),
  setActiveWorkspaceId: vi.fn(),
}));

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
  isTauri: () => false,
  resetBackend: mocks.resetBackend,
}));

vi.mock("$lib/auth", () => ({
  setActiveWorkspaceId: mocks.setActiveWorkspaceId,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  removeLocalWorkspace: mocks.removeLocalWorkspace,
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  loadAllPlugins: mocks.loadAllPlugins,
}));

vi.mock("$lib/plugins/pluginRegistry", () => ({
  fetchPluginRegistry: mocks.fetchPluginRegistry,
}));

vi.mock("$lib/plugins/pluginInstallService", () => ({
  installRegistryPlugin: mocks.installRegistryPlugin,
}));

vi.mock("$lib/sync/workspaceProviderService", () => ({
  downloadWorkspace: mocks.downloadWorkspace,
  linkWorkspace: vi.fn(),
}));

import { handleCreateWithProvider } from "./onboardingController";

describe("onboardingController", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("infers registry plugins from restored workspace frontmatter", async () => {
    const providerOverrideBytes = new Uint8Array([1, 2, 3, 4]).buffer;
    const pluginConfig = new Map<string, unknown>([
      ["diaryx.sync", new Map<string, unknown>()],
      ["diaryx.daily", { enabled: true }],
      ["custom.local", { enabled: true }],
    ]);
    const frontmatter = new Map<string, unknown>([
      ["plugins", pluginConfig],
      ["disabled_plugins", ["diaryx.hidden"]],
    ]);
    const restoredBackend = {
      getWorkspacePath: vi.fn(() => "/workspace/README.md"),
    };
    const api = {
      resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("/workspace/README.md"),
      getFrontmatter: vi.fn().mockResolvedValue(frontmatter),
    };

    mocks.downloadWorkspace.mockResolvedValue({
      localId: "workspace-1",
      filesImported: 7,
    });
    mocks.getBackend.mockResolvedValue(restoredBackend);
    mocks.fetchPluginRegistry.mockResolvedValue({
      plugins: [
        { id: "diaryx.sync", name: "Sync" },
        { id: "diaryx.daily", name: "Daily" },
        { id: "diaryx.hidden", name: "Hidden" },
      ],
    });

    const deps = {
      autoCreateDeps: {
        clearRustApi: vi.fn(),
        createApi: vi.fn(() => api),
        initEventSubscription: vi.fn(() => () => {}),
        persistPermissionDefaults: vi.fn(),
        setBackend: vi.fn(),
        setCleanupEventSubscription: vi.fn(),
      },
      dismissLaunchOverlay: vi.fn(),
      expandNode: vi.fn(),
      getTree: vi.fn(() => ({ path: "/workspace/README.md" })),
      installLocalPlugin: vi.fn().mockResolvedValue(undefined),
      openEntry: vi.fn().mockResolvedValue(undefined),
      persistPermissionDefaults: vi.fn(),
      refreshTree: vi.fn().mockResolvedValue(undefined),
      runValidation: vi.fn().mockResolvedValue(undefined),
      switchWorkspace: vi.fn(),
    };

    const result = await handleCreateWithProvider(
      deps as any,
      null,
      "diaryx.sync",
      [{
        targetPluginId: "diaryx.sync",
        fileName: "sync-local.wasm",
        bytes: providerOverrideBytes,
      }],
      {
        id: "remote-1",
        metadata: {
          name: "Restored Workspace",
          provider: "diaryx.sync",
        },
      },
    );

    expect(mocks.downloadWorkspace).toHaveBeenCalledWith(
      "diaryx.sync",
      {
        remoteId: "remote-1",
        name: "Restored Workspace",
        link: true,
      },
      undefined,
      undefined,
      expect.any(Uint8Array),
    );
    expect(mocks.setActiveWorkspaceId).toHaveBeenCalledWith("workspace-1");
    expect(mocks.loadAllPlugins).toHaveBeenCalledTimes(1);
    expect(deps.installLocalPlugin).toHaveBeenCalledWith(
      providerOverrideBytes,
      "sync-local",
    );
    expect(api.resolveWorkspaceRootIndexPath).toHaveBeenCalledWith("/workspace/README.md");
    expect(mocks.installRegistryPlugin).toHaveBeenCalledTimes(2);
    expect(mocks.installRegistryPlugin).toHaveBeenNthCalledWith(1, {
      id: "diaryx.daily",
      name: "Daily",
    });
    expect(mocks.installRegistryPlugin).toHaveBeenNthCalledWith(2, {
      id: "diaryx.hidden",
      name: "Hidden",
    });
    expect(result).toEqual({ spotlightSteps: null });
  });
});
