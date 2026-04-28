import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const backend = { kind: "backend" };
  const api = { kind: "api" };
  const pluginStore = {
    init: vi.fn(async () => {}),
  };

  return {
    backend,
    api,
    pluginStore,
    getBackend: vi.fn(async () => backend),
    resetBackend: vi.fn(),
    createApi: vi.fn(() => api),
    setBackend: vi.fn(),
    setCurrentWorkspaceId: vi.fn(),
    setActiveWorkspaceId: vi.fn(),
    getWorkspaceStorageType: vi.fn(() => "opfs"),
    getWorkspaceStoragePluginId: vi.fn(() => null),
    hydrateProviderLinksFromFrontmatter: vi.fn(async () => {}),
  };
});

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
  resetBackend: mocks.resetBackend,
}));

vi.mock("$lib/backend/api", () => ({
  createApi: mocks.createApi,
}));

vi.mock("@/models/stores/pluginStore.svelte", () => ({
  getPluginStore: () => mocks.pluginStore,
}));

vi.mock("@/models/stores/workspaceStore.svelte", () => ({
  workspaceStore: {
    setBackend: mocks.setBackend,
  },
}));

vi.mock("$lib/auth", () => ({
  setActiveWorkspaceId: mocks.setActiveWorkspaceId,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  setCurrentWorkspaceId: mocks.setCurrentWorkspaceId,
  getWorkspaceStorageType: mocks.getWorkspaceStorageType,
  getWorkspaceStoragePluginId: mocks.getWorkspaceStoragePluginId,
}));

vi.mock("./hydrateProviderLinks", () => ({
  hydrateProviderLinksFromFrontmatter: mocks.hydrateProviderLinksFromFrontmatter,
}));

import { switchWorkspace } from "./switchWorkspace";

describe("switchWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getBackend.mockResolvedValue(mocks.backend);
    mocks.createApi.mockReturnValue(mocks.api);
    mocks.pluginStore.init.mockResolvedValue(undefined);
    mocks.getWorkspaceStorageType.mockReturnValue("opfs");
    mocks.getWorkspaceStoragePluginId.mockReturnValue(null);
    mocks.hydrateProviderLinksFromFrontmatter.mockResolvedValue(undefined);
  });

  it("rebuilds backend state and refreshes plugin manifests for the new workspace", async () => {
    await switchWorkspace("workspace-2", "Workspace Two");

    expect(mocks.setCurrentWorkspaceId).toHaveBeenCalledWith("workspace-2");
    expect(mocks.setActiveWorkspaceId).toHaveBeenCalledWith("workspace-2");
    expect(mocks.resetBackend).toHaveBeenCalledOnce();
    expect(mocks.getBackend).toHaveBeenCalledWith(
      "workspace-2",
      "Workspace Two",
      "opfs",
      null,
    );
    expect(mocks.setBackend).toHaveBeenCalledWith(mocks.backend);
    expect(mocks.createApi).toHaveBeenCalledWith(mocks.backend);
    expect(mocks.pluginStore.init).toHaveBeenCalledWith(mocks.api);
  });

  it("waits for plugin manifest refresh before reporting readiness", async () => {
    const sequence: string[] = [];
    mocks.pluginStore.init.mockImplementation(async () => {
      sequence.push("plugin-init");
    });
    mocks.hydrateProviderLinksFromFrontmatter.mockImplementation(async () => {
      sequence.push("hydrate");
    });

    await switchWorkspace("workspace-2", "Workspace Two", {
      onReady: () => {
        sequence.push("ready");
      },
    });

    expect(sequence).toEqual(["plugin-init", "hydrate", "ready"]);
  });

  it("hydrates provider links from frontmatter after plugin init", async () => {
    await switchWorkspace("workspace-2", "Workspace Two");

    expect(mocks.hydrateProviderLinksFromFrontmatter).toHaveBeenCalledWith(
      "workspace-2",
      mocks.api,
      mocks.backend,
    );
  });
});
