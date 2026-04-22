import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  downloadWorkspace: vi.fn(),
  fetchPluginRegistry: vi.fn(),
  fetchStarterWorkspaceRegistry: vi.fn(),
  fetchStarterWorkspaceZip: vi.fn(),
  fetchThemeRegistry: vi.fn(),
  fetchTypographyRegistry: vi.fn(),
  getBackend: vi.fn(),
  hydrateOnboardingPluginPermissionDefaults: vi.fn(),
  installRegistryPlugin: vi.fn(),
  isTauri: vi.fn(() => false),
  isIOS: vi.fn(() => false),
  linkWorkspace: vi.fn(),
  loadAllPlugins: vi.fn(),
  planBundleApply: vi.fn(),
  executeBundleApply: vi.fn(),
  createDefaultBundleApplyRuntime: vi.fn(),
  proxyFetch: vi.fn(),
  removeLocalWorkspace: vi.fn(),
  resetBackend: vi.fn(),
  resolveStorageType: vi.fn(async () => "opfs" as const),
  setActiveWorkspaceId: vi.fn(),
}));

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
  isTauri: mocks.isTauri,
  resetBackend: mocks.resetBackend,
}));

vi.mock("$lib/auth", () => ({
  setActiveWorkspaceId: mocks.setActiveWorkspaceId,
}));

vi.mock("$lib/backend/storageType", () => ({
  resolveStorageType: mocks.resolveStorageType,
}));

vi.mock("$lib/hooks/useMobile.svelte", () => ({
  isIOS: mocks.isIOS,
}));

vi.mock("$lib/storage/localWorkspaceRegistry.svelte", () => ({
  removeLocalWorkspace: mocks.removeLocalWorkspace,
  getCurrentWorkspaceId: vi.fn(() => null),
  getLocalWorkspaces: vi.fn(() => []),
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
  linkWorkspace: mocks.linkWorkspace,
}));

vi.mock("$lib/sync/builtinProviders", () => ({
  isBuiltinProvider: (id: string) => id.startsWith("builtin."),
  BUILTIN_ICLOUD_PROVIDER_ID: "builtin.icloud",
}));

vi.mock("$lib/marketplace/starterWorkspaceRegistry", () => ({
  fetchStarterWorkspaceRegistry: mocks.fetchStarterWorkspaceRegistry,
}));

vi.mock("$lib/marketplace/starterWorkspaceApply", () => ({
  fetchStarterWorkspaceZip: mocks.fetchStarterWorkspaceZip,
}));

vi.mock("$lib/marketplace/bundleApply", () => ({
  planBundleApply: mocks.planBundleApply,
  executeBundleApply: mocks.executeBundleApply,
  createDefaultBundleApplyRuntime: mocks.createDefaultBundleApplyRuntime,
}));

vi.mock("$lib/marketplace/onboardingPluginPermissions", () => ({
  hydrateOnboardingPluginPermissionDefaults: mocks.hydrateOnboardingPluginPermissionDefaults,
}));

vi.mock("$lib/marketplace/themeRegistry", () => ({
  fetchThemeRegistry: mocks.fetchThemeRegistry,
}));

vi.mock("$lib/marketplace/typographyRegistry", () => ({
  fetchTypographyRegistry: mocks.fetchTypographyRegistry,
}));

vi.mock("$lib/backend/proxyFetch", () => ({
  proxyFetch: mocks.proxyFetch,
}));

import {
  getWorkspaceDirectoryPath,
  isWorkspaceAlreadyExistsError,
  shouldBypassWelcomeScreenForE2E,
  seedStarterWorkspaceContent,
  maybeBootstrapIosStarterWorkspace,
  applyOnboardingBundle,
  autoCreateDefaultWorkspace,
  handleGetStarted,
  handleSignInCreateNew,
  handleCreateWithProvider,
  handleWelcomeComplete,
} from "./onboardingController";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeAutoCreateDeps(overrides: Record<string, any> = {}) {
  const backend = {
    getWorkspacePath: vi.fn(() => "/workspace/index.md"),
    importFromZip: vi.fn().mockResolvedValue(undefined),
    ...overrides.backend,
  };
  const api = {
    createWorkspace: vi.fn().mockResolvedValue(undefined),
    findRootIndex: vi.fn().mockResolvedValue("/workspace/index.md"),
    saveEntry: vi.fn().mockResolvedValue(undefined),
    createChildEntry: vi.fn().mockResolvedValue({ child_path: "/workspace/Untitled.md" }),
    setFrontmatterProperty: vi.fn().mockResolvedValue("/workspace/Detailed Guide.md"),
    getEntry: vi.fn().mockResolvedValue({
      content: "",
      frontmatter: {},
    }),
    resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("/workspace/index.md"),
    getFrontmatter: vi.fn().mockResolvedValue({}),
    getFilesystemTree: vi.fn().mockResolvedValue({ children: [] }),
    ...overrides.api,
  };
  return {
    deps: {
      createLocalWorkspace: vi.fn(() => ({ id: "ws-1", name: "My Workspace", storageType: "opfs" })),
      setCurrentWorkspaceId: vi.fn(),
      getBackend: vi.fn().mockResolvedValue(backend),
      createApi: vi.fn(() => api),
      setBackend: vi.fn(),
      initEventSubscription: vi.fn(() => () => {}),
      setCleanupEventSubscription: vi.fn(),
      refreshTree: vi.fn().mockResolvedValue(undefined),
      setupPermissions: vi.fn(),
      persistPermissionDefaults: vi.fn().mockResolvedValue(undefined),
      ...overrides.deps,
    },
    backend,
    api,
  };
}

function makeOnGetStartedDeps(overrides: Record<string, any> = {}) {
  const { deps: autoCreateDeps, backend, api } = makeAutoCreateDeps(overrides);
  return {
    deps: {
      autoCreateDeps,
      installLocalPlugin: vi.fn().mockResolvedValue(undefined),
      refreshTree: vi.fn().mockResolvedValue(undefined),
      getTree: vi.fn(() => ({ path: "/workspace/index.md" })),
      expandNode: vi.fn(),
      openEntry: vi.fn().mockResolvedValue(undefined),
      runValidation: vi.fn().mockResolvedValue(undefined),
      dismissLaunchOverlay: vi.fn().mockResolvedValue(undefined),
      ...overrides.getStartedDeps,
    },
    backend,
    api,
  };
}

function makeCreateWithProviderDeps(overrides: Record<string, any> = {}) {
  const { deps: autoCreateDeps, backend, api } = makeAutoCreateDeps(overrides);
  return {
    deps: {
      autoCreateDeps,
      installLocalPlugin: vi.fn().mockResolvedValue(undefined),
      refreshTree: vi.fn().mockResolvedValue(undefined),
      getTree: vi.fn(() => ({ path: "/workspace/index.md" })),
      expandNode: vi.fn(),
      openEntry: vi.fn().mockResolvedValue(undefined),
      runValidation: vi.fn().mockResolvedValue(undefined),
      dismissLaunchOverlay: vi.fn().mockResolvedValue(undefined),
      persistPermissionDefaults: vi.fn().mockResolvedValue(undefined),
      switchWorkspace: vi.fn().mockResolvedValue(undefined),
      ...overrides.providerDeps,
    },
    backend,
    api,
  };
}

// ===========================================================================
// Tests
// ===========================================================================

describe("onboardingController", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.isTauri.mockReturnValue(false);
    mocks.isIOS.mockReturnValue(false);
    mocks.resolveStorageType.mockResolvedValue("opfs");
    mocks.executeBundleApply.mockResolvedValue({
      summary: { success: 0, failed: 0, total: 0 },
      results: [],
    });
    mocks.createDefaultBundleApplyRuntime.mockReturnValue({});
    mocks.planBundleApply.mockReturnValue({ steps: [] });
    mocks.fetchThemeRegistry.mockResolvedValue({ themes: [] });
    mocks.fetchTypographyRegistry.mockResolvedValue({ typographies: [] });
    mocks.fetchPluginRegistry.mockResolvedValue({ plugins: [] });
    mocks.hydrateOnboardingPluginPermissionDefaults.mockResolvedValue(undefined);
  });

  // -------------------------------------------------------------------------
  // getWorkspaceDirectoryPath
  // -------------------------------------------------------------------------

  describe("getWorkspaceDirectoryPath", () => {
    it("strips trailing /index.md", () => {
      const backend = { getWorkspacePath: () => "/my/workspace/index.md" };
      expect(getWorkspaceDirectoryPath(backend)).toBe("/my/workspace");
    });

    it("strips trailing /README.md", () => {
      const backend = { getWorkspacePath: () => "/my/workspace/README.md" };
      expect(getWorkspaceDirectoryPath(backend)).toBe("/my/workspace");
    });

    it("returns path unchanged when no index/README suffix", () => {
      const backend = { getWorkspacePath: () => "/my/workspace" };
      expect(getWorkspaceDirectoryPath(backend)).toBe("/my/workspace");
    });
  });

  // -------------------------------------------------------------------------
  // isWorkspaceAlreadyExistsError
  // -------------------------------------------------------------------------

  describe("isWorkspaceAlreadyExistsError", () => {
    it("returns true for Error with 'Workspace already exists'", () => {
      expect(isWorkspaceAlreadyExistsError(new Error("Workspace already exists at /path"))).toBe(true);
    });

    it("returns true for Error with 'WorkspaceAlreadyExists'", () => {
      expect(isWorkspaceAlreadyExistsError(new Error("WorkspaceAlreadyExists"))).toBe(true);
    });

    it("returns true for string matching the pattern", () => {
      expect(isWorkspaceAlreadyExistsError("Workspace already exists")).toBe(true);
    });

    it("returns false for unrelated error", () => {
      expect(isWorkspaceAlreadyExistsError(new Error("Something else"))).toBe(false);
    });

    it("returns false for null/undefined", () => {
      expect(isWorkspaceAlreadyExistsError(null)).toBe(false);
      expect(isWorkspaceAlreadyExistsError(undefined)).toBe(false);
    });
  });

  // -------------------------------------------------------------------------
  // shouldBypassWelcomeScreenForE2E
  // -------------------------------------------------------------------------

  describe("shouldBypassWelcomeScreenForE2E", () => {
    it("returns false in non-DEV mode", () => {
      // import.meta.env.DEV is false in test mode by default in vitest
      // so this should return false
      expect(shouldBypassWelcomeScreenForE2E()).toBe(false);
    });
  });

  // -------------------------------------------------------------------------
  // seedStarterWorkspaceContent
  // -------------------------------------------------------------------------

  describe("seedStarterWorkspaceContent", () => {
    it("creates workspace and seeds starter content", async () => {
      const api = {
        createWorkspace: vi.fn().mockResolvedValue(undefined),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue("/ws/Detailed Guide.md"),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "My Workspace");
      expect(result).toBe("/ws/index.md");
      expect(api.createWorkspace).toHaveBeenCalledWith("/ws", "My Workspace");
      expect(api.saveEntry).toHaveBeenCalledTimes(2);
      expect(api.createChildEntry).toHaveBeenCalledWith("/ws/index.md");
      expect(api.setFrontmatterProperty).toHaveBeenCalledWith(
        "/ws/Untitled.md",
        "title",
        "Detailed Guide",
        "/ws/index.md",
      );
    });

    it("seeds content when setFrontmatterProperty returns null (no rename)", async () => {
      const api = {
        createWorkspace: vi.fn().mockResolvedValue(undefined),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue(null),
      } as any;

      await seedStarterWorkspaceContent(api, "/ws", "Test");
      // When setFrontmatterProperty returns null, the original child path is used
      expect(api.saveEntry).toHaveBeenCalledTimes(2);
      expect(api.saveEntry).toHaveBeenNthCalledWith(
        2,
        "/ws/Untitled.md",
        expect.any(String),
        "/ws/index.md",
      );
    });

    it("tolerates WorkspaceAlreadyExists and detects pristine scaffold", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Workspace already exists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockResolvedValue({
          content: "# My WS\n\nA diaryx workspace",
          frontmatter: { title: "My WS", description: "A diaryx workspace", contents: [] },
        }),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue("/ws/Guide.md"),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "My WS");
      expect(result).toBe("/ws/index.md");
      // Pristine scaffold detected -> should seed content
      expect(api.saveEntry).toHaveBeenCalledTimes(2);
    });

    it("skips seeding for already-initialized workspace", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Workspace already exists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockResolvedValue({
          content: "# My WS\n\nCustom user content here",
          frontmatter: { title: "My WS", description: "My custom workspace" },
        }),
        saveEntry: vi.fn(),
        createChildEntry: vi.fn(),
        setFrontmatterProperty: vi.fn(),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "My WS");
      expect(result).toBe("/ws/index.md");
      // Should NOT seed because the workspace is already customized
      expect(api.saveEntry).not.toHaveBeenCalled();
    });

    it("skips seeding when getEntry throws on existing workspace", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("WorkspaceAlreadyExists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockRejectedValue(new Error("not found")),
        saveEntry: vi.fn(),
        createChildEntry: vi.fn(),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "My WS");
      expect(result).toBe("/ws/index.md");
      expect(api.saveEntry).not.toHaveBeenCalled();
    });

    it("re-throws non-WorkspaceAlreadyExists errors from createWorkspace", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Disk full")),
        findRootIndex: vi.fn(),
      } as any;

      await expect(seedStarterWorkspaceContent(api, "/ws", "Test")).rejects.toThrow("Disk full");
    });

    it("handles frontmatter with Map instances", async () => {
      const fm = new Map<string, any>([
        ["title", "My WS"],
        ["description", "A diaryx workspace"],
        ["contents", []],
      ]);
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Workspace already exists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockResolvedValue({
          content: "# My WS\n\nA diaryx workspace",
          frontmatter: fm,
        }),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue(null),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "My WS");
      expect(result).toBe("/ws/index.md");
      // Pristine scaffold detected with Map frontmatter
      expect(api.saveEntry).toHaveBeenCalledTimes(2);
    });

    it("detects pristine scaffold when title falls back to workspace name", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Workspace already exists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockResolvedValue({
          content: "# FallbackName\n\nA diaryx workspace",
          frontmatter: { title: "", description: "A diaryx workspace", contents: [] },
        }),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue(null),
      } as any;

      const result = await seedStarterWorkspaceContent(api, "/ws", "FallbackName");
      expect(result).toBe("/ws/index.md");
      // Title was empty, falls back to workspaceName -> matches default body
      expect(api.saveEntry).toHaveBeenCalledTimes(2);
    });

    it("does not re-seed when contents array is non-empty", async () => {
      const api = {
        createWorkspace: vi.fn().mockRejectedValue(new Error("Workspace already exists")),
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
        getEntry: vi.fn().mockResolvedValue({
          content: "# My WS\n\nA diaryx workspace",
          frontmatter: { title: "My WS", description: "A diaryx workspace", contents: ["child.md"] },
        }),
        saveEntry: vi.fn(),
      } as any;

      await seedStarterWorkspaceContent(api, "/ws", "My WS");
      expect(api.saveEntry).not.toHaveBeenCalled();
    });
  });

  // -------------------------------------------------------------------------
  // maybeBootstrapIosStarterWorkspace
  // -------------------------------------------------------------------------

  describe("maybeBootstrapIosStarterWorkspace", () => {
    it("returns false when not Tauri + iOS", async () => {
      mocks.isTauri.mockReturnValue(false);
      mocks.isIOS.mockReturnValue(false);
      const result = await maybeBootstrapIosStarterWorkspace({} as any, { getWorkspacePath: () => "/ws/index.md" } as any, "Test");
      expect(result).toBe(false);
    });

    it("returns false when only Tauri (not iOS)", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(false);
      const result = await maybeBootstrapIosStarterWorkspace({} as any, { getWorkspacePath: () => "/ws/index.md" } as any, "Test");
      expect(result).toBe(false);
    });

    it("returns false when root index already exists", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(true);
      const api = {
        findRootIndex: vi.fn().mockResolvedValue("/ws/index.md"),
      } as any;
      const backend = { getWorkspacePath: () => "/ws/index.md" } as any;

      const result = await maybeBootstrapIosStarterWorkspace(api, backend, "Test");
      expect(result).toBe(false);
    });

    it("returns false when workspace has files but no root index", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(true);
      const api = {
        findRootIndex: vi.fn().mockRejectedValue(new Error("not found")),
        getFilesystemTree: vi.fn().mockResolvedValue({ children: [{ name: "note.md" }] }),
      } as any;
      const backend = { getWorkspacePath: () => "/ws/index.md" } as any;

      const result = await maybeBootstrapIosStarterWorkspace(api, backend, "Test");
      expect(result).toBe(false);
    });

    it("bootstraps and returns true when empty iOS workspace", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(true);
      const api = {
        findRootIndex: vi.fn()
          .mockRejectedValueOnce(new Error("not found"))
          .mockResolvedValue("/ws/index.md"),
        getFilesystemTree: vi.fn().mockResolvedValue({ children: [] }),
        createWorkspace: vi.fn().mockResolvedValue(undefined),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue(null),
      } as any;
      const backend = { getWorkspacePath: () => "/ws/index.md" } as any;

      const result = await maybeBootstrapIosStarterWorkspace(api, backend, "Test");
      expect(result).toBe(true);
    });

    it("returns false when bootstrap throws", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(true);
      const api = {
        findRootIndex: vi.fn().mockRejectedValue(new Error("not found")),
        getFilesystemTree: vi.fn().mockRejectedValue(new Error("fs error")),
      } as any;
      const backend = { getWorkspacePath: () => "/ws/index.md" } as any;

      const result = await maybeBootstrapIosStarterWorkspace(api, backend, "Test");
      expect(result).toBe(false);
    });

    it("handles filesystem tree with null children", async () => {
      mocks.isTauri.mockReturnValue(true);
      mocks.isIOS.mockReturnValue(true);
      const api = {
        findRootIndex: vi.fn()
          .mockRejectedValueOnce(new Error("not found"))
          .mockResolvedValue("/ws/index.md"),
        getFilesystemTree: vi.fn().mockResolvedValue({ children: null }),
        createWorkspace: vi.fn().mockResolvedValue(undefined),
        saveEntry: vi.fn().mockResolvedValue(undefined),
        createChildEntry: vi.fn().mockResolvedValue({ child_path: "/ws/Untitled.md" }),
        setFrontmatterProperty: vi.fn().mockResolvedValue(null),
      } as any;
      const backend = { getWorkspacePath: () => "/ws/index.md" } as any;

      const result = await maybeBootstrapIosStarterWorkspace(api, backend, "Test");
      expect(result).toBe(true);
    });
  });

  // -------------------------------------------------------------------------
  // applyOnboardingBundle
  // -------------------------------------------------------------------------

  describe("applyOnboardingBundle", () => {
    it("fetches registries and executes bundle plan", async () => {
      const bundle = { plugins: [{ plugin_id: "p1" }] } as any;
      const persistFn = vi.fn();

      await applyOnboardingBundle(bundle, persistFn);

      expect(mocks.fetchThemeRegistry).toHaveBeenCalled();
      expect(mocks.fetchTypographyRegistry).toHaveBeenCalled();
      expect(mocks.fetchPluginRegistry).toHaveBeenCalled();
      expect(mocks.hydrateOnboardingPluginPermissionDefaults).toHaveBeenCalledWith(
        bundle.plugins,
        [],
        persistFn,
      );
      expect(mocks.planBundleApply).toHaveBeenCalledWith(bundle, {
        themes: [],
        typographies: [],
        plugins: [],
      });
      expect(mocks.executeBundleApply).toHaveBeenCalled();
    });

    it("uses fallback values when registry fetches fail", async () => {
      mocks.fetchThemeRegistry.mockRejectedValue(new Error("network"));
      mocks.fetchTypographyRegistry.mockRejectedValue(new Error("network"));
      mocks.fetchPluginRegistry.mockRejectedValue(new Error("network"));

      const bundle = { plugins: [] } as any;
      await applyOnboardingBundle(bundle, vi.fn());

      expect(mocks.planBundleApply).toHaveBeenCalledWith(bundle, {
        themes: [],
        typographies: [],
        plugins: [],
      });
    });

    it("logs warning when some bundle steps fail", async () => {
      mocks.executeBundleApply.mockResolvedValue({
        summary: { success: 1, failed: 1, total: 2 },
        results: [
          { status: "success" },
          { status: "failed", error: "download error" },
        ],
      });
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const bundle = { plugins: [{ plugin_id: "p1" }] } as any;
      await applyOnboardingBundle(bundle, vi.fn());

      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining("1/2 succeeded"),
        expect.any(Array),
      );
      warnSpy.mockRestore();
    });
  });

  // -------------------------------------------------------------------------
  // autoCreateDefaultWorkspace
  // -------------------------------------------------------------------------

  describe("autoCreateDefaultWorkspace", () => {
    it("creates workspace with starter content when no bundle", async () => {
      const { deps, api } = makeAutoCreateDeps();

      const result = await autoCreateDefaultWorkspace(deps as any, null);

      expect(result).toEqual({ id: "ws-1", name: "My Workspace" });
      expect(deps.createLocalWorkspace).toHaveBeenCalledWith("My Workspace", "opfs");
      expect(deps.setCurrentWorkspaceId).toHaveBeenCalledWith("ws-1");
      expect(api.createWorkspace).toHaveBeenCalled();
      expect(deps.refreshTree).toHaveBeenCalled();
      expect(deps.setupPermissions).toHaveBeenCalled();
    });

    it("imports starter workspace from bundle when starter_workspace_id is provided", async () => {
      const zipBlob = new Blob(["zip"]);
      mocks.fetchStarterWorkspaceRegistry.mockResolvedValue({
        starters: [{ id: "starter-1", artifact: { url: "https://example.com/starter.zip" } }],
      });
      mocks.fetchStarterWorkspaceZip.mockResolvedValue(zipBlob);

      const { deps, backend, api } = makeAutoCreateDeps();

      const bundle = {
        starter_workspace_id: "starter-1",
        plugins: [],
      } as any;

      await autoCreateDefaultWorkspace(deps as any, bundle);

      expect(mocks.fetchStarterWorkspaceRegistry).toHaveBeenCalled();
      expect(mocks.fetchStarterWorkspaceZip).toHaveBeenCalled();
      expect(backend.importFromZip).toHaveBeenCalled();
      // Should NOT call seedStarterWorkspaceContent since import succeeded
      expect(api.createWorkspace).not.toHaveBeenCalled();
    });

    it("falls back to seed content when starter workspace import fails", async () => {
      mocks.fetchStarterWorkspaceRegistry.mockRejectedValue(new Error("network error"));
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const { deps, api } = makeAutoCreateDeps();

      const bundle = {
        starter_workspace_id: "starter-1",
        plugins: [],
      } as any;

      await autoCreateDefaultWorkspace(deps as any, bundle);

      // Should fall back to seeding starter content
      expect(api.createWorkspace).toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("falls back when starter not found in registry", async () => {
      mocks.fetchStarterWorkspaceRegistry.mockResolvedValue({
        starters: [{ id: "other-starter", artifact: { url: "https://example.com" } }],
      });

      const { deps, api } = makeAutoCreateDeps();

      const bundle = {
        starter_workspace_id: "nonexistent",
        plugins: [],
      } as any;

      await autoCreateDefaultWorkspace(deps as any, bundle);

      // starter not found -> falls back to seed
      expect(api.createWorkspace).toHaveBeenCalled();
    });

    it("applies bundle plugins after workspace creation", async () => {
      const { deps } = makeAutoCreateDeps();

      const bundle = {
        plugins: [{ plugin_id: "p1" }],
      } as any;

      await autoCreateDefaultWorkspace(deps as any, bundle);

      expect(mocks.planBundleApply).toHaveBeenCalled();
      expect(mocks.executeBundleApply).toHaveBeenCalled();
    });

    it("does not apply bundle when plugins array is empty", async () => {
      const { deps } = makeAutoCreateDeps();

      const bundle = { plugins: [] } as any;

      await autoCreateDefaultWorkspace(deps as any, bundle);

      expect(mocks.planBundleApply).not.toHaveBeenCalled();
    });

    it("tolerates bundle apply failure (non-fatal)", async () => {
      mocks.executeBundleApply.mockRejectedValue(new Error("install error"));
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const { deps } = makeAutoCreateDeps();

      const bundle = { plugins: [{ plugin_id: "p1" }] } as any;

      // Should not throw
      const result = await autoCreateDefaultWorkspace(deps as any, bundle);
      expect(result).toEqual({ id: "ws-1", name: "My Workspace" });
      warnSpy.mockRestore();
    });

    it("rolls back on fatal error during workspace setup", async () => {
      const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
      const { deps } = makeAutoCreateDeps();
      deps.getBackend = vi.fn().mockRejectedValue(new Error("backend init failed"));

      await expect(autoCreateDefaultWorkspace(deps as any, null)).rejects.toThrow("backend init failed");
      expect(mocks.removeLocalWorkspace).toHaveBeenCalledWith("ws-1");
      expect(mocks.resetBackend).toHaveBeenCalled();
      errorSpy.mockRestore();
    });
  });

  // -------------------------------------------------------------------------
  // handleGetStarted
  // -------------------------------------------------------------------------

  describe("handleGetStarted", () => {
    it("creates workspace, refreshes tree, opens root entry", async () => {
      const { deps } = makeOnGetStartedDeps();

      const result = await handleGetStarted(deps as any, null, null);

      expect(deps.refreshTree).toHaveBeenCalled();
      expect(deps.getTree).toHaveBeenCalled();
      expect(deps.expandNode).toHaveBeenCalledWith("/workspace/index.md");
      expect(deps.openEntry).toHaveBeenCalledWith("/workspace/index.md");
      expect(deps.runValidation).toHaveBeenCalled();
      expect(deps.dismissLaunchOverlay).toHaveBeenCalled();
      expect(result.spotlightSteps).toBeNull();
    });

    it("returns spotlight steps from bundle", async () => {
      const { deps } = makeOnGetStartedDeps();

      const bundle = {
        plugins: [],
        spotlight: [{ step: 1 }, { step: 2 }],
      } as any;

      const result = await handleGetStarted(deps as any, bundle, null);
      expect(result.spotlightSteps).toEqual([{ step: 1 }, { step: 2 }]);
    });

    it("returns null spotlight for empty spotlight array", async () => {
      const { deps } = makeOnGetStartedDeps();

      const bundle = { plugins: [], spotlight: [] } as any;

      const result = await handleGetStarted(deps as any, bundle, null);
      expect(result.spotlightSteps).toBeNull();
    });

    it("installs plugin overrides", async () => {
      const { deps } = makeOnGetStartedDeps();
      const overrideBytes = new ArrayBuffer(4);

      await handleGetStarted(deps as any, null, [
        { targetPluginId: "p1", bytes: overrideBytes, fileName: "myplugin.wasm" },
      ]);

      expect(deps.installLocalPlugin).toHaveBeenCalledWith(overrideBytes, "myplugin");
    });

    it("excludes overridden plugins from bundle before creation", async () => {
      const { deps } = makeOnGetStartedDeps();
      const bundle = {
        plugins: [{ plugin_id: "p1" }, { plugin_id: "p2" }],
      } as any;
      const overrides = [
        { targetPluginId: "p1", bytes: new ArrayBuffer(4), fileName: "p1.wasm" },
      ];

      await handleGetStarted(deps as any, bundle, overrides);

      // autoCreateDefaultWorkspace should have received a bundle without p1
      // We verify indirectly: installLocalPlugin was called for the override
      expect(deps.installLocalPlugin).toHaveBeenCalledWith(overrides[0].bytes, "p1");
    });

    it("skips tree navigation when getTree returns null", async () => {
      const { deps } = makeOnGetStartedDeps({
        getStartedDeps: {
          getTree: vi.fn(() => null),
        },
      });

      await handleGetStarted(deps as any, null, null);

      expect(deps.expandNode).not.toHaveBeenCalled();
      expect(deps.openEntry).not.toHaveBeenCalled();
    });
  });

  // -------------------------------------------------------------------------
  // handleSignInCreateNew
  // -------------------------------------------------------------------------

  describe("handleSignInCreateNew", () => {
    it("creates workspace, opens root entry", async () => {
      const { deps: autoCreateDeps } = makeAutoCreateDeps();
      const deps = {
        autoCreateDeps,
        refreshTree: vi.fn().mockResolvedValue(undefined),
        getTree: vi.fn(() => ({ path: "/workspace/index.md" })),
        expandNode: vi.fn(),
        openEntry: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
      };

      await handleSignInCreateNew(deps as any);

      expect(deps.refreshTree).toHaveBeenCalled();
      expect(deps.expandNode).toHaveBeenCalledWith("/workspace/index.md");
      expect(deps.openEntry).toHaveBeenCalledWith("/workspace/index.md");
      expect(deps.runValidation).toHaveBeenCalled();
    });

    it("skips tree navigation when getTree returns null", async () => {
      const { deps: autoCreateDeps } = makeAutoCreateDeps();
      const deps = {
        autoCreateDeps,
        refreshTree: vi.fn().mockResolvedValue(undefined),
        getTree: vi.fn(() => null),
        expandNode: vi.fn(),
        openEntry: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
      };

      await handleSignInCreateNew(deps as any);

      expect(deps.expandNode).not.toHaveBeenCalled();
      expect(deps.openEntry).not.toHaveBeenCalled();
    });
  });

  // -------------------------------------------------------------------------
  // handleCreateWithProvider
  // -------------------------------------------------------------------------

  describe("handleCreateWithProvider", () => {
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

    it("creates new workspace with provider link when no restoreNamespace", async () => {
      const { deps } = makeCreateWithProviderDeps();

      const result = await handleCreateWithProvider(
        deps as any,
        { plugins: [] } as any,
        "my-sync-plugin",
        null,
        null,
      );

      expect(mocks.linkWorkspace).toHaveBeenCalledWith(
        "my-sync-plugin",
        { localId: "ws-1", name: "My Workspace" },
      );
      expect(result.spotlightSteps).toBeNull();
    });

    it("returns spotlight steps from bundle when no restoreNamespace", async () => {
      const { deps } = makeCreateWithProviderDeps();

      const bundle = {
        plugins: [],
        spotlight: [{ step: "intro" }],
      } as any;

      const result = await handleCreateWithProvider(
        deps as any,
        bundle,
        null,
        null,
        null,
      );

      expect(result.spotlightSteps).toEqual([{ step: "intro" }]);
    });

    it("skips link when no providerPluginId on new workspace path", async () => {
      const { deps } = makeCreateWithProviderDeps();

      await handleCreateWithProvider(
        deps as any,
        { plugins: [] } as any,
        null,
        null,
        null,
      );

      expect(mocks.linkWorkspace).not.toHaveBeenCalled();
    });

    it("uses builtin provider (null wasm) for restore path", async () => {
      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 3 });
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      mocks.getBackend.mockResolvedValue(restoredBackend);

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue(null),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await handleCreateWithProvider(
        deps as any,
        null,
        "builtin.icloud",
        null,
        { id: "remote-1", metadata: { name: "iCloud WS" } },
      );

      expect(mocks.downloadWorkspace).toHaveBeenCalledWith(
        "builtin.icloud",
        expect.objectContaining({ remoteId: "remote-1", name: "iCloud WS" }),
        undefined,
        undefined,
        null,
      );
    });

    it("throws user-friendly error when download is unsupported", async () => {
      mocks.downloadWorkspace.mockRejectedValue(
        new Error("only available in host-integrated runtimes"),
      );

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn(),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "builtin.icloud",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("does not support downloading workspaces");
    });

    it("re-throws non-unsupported download errors", async () => {
      mocks.downloadWorkspace.mockRejectedValue(new Error("network timeout"));

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn(),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "builtin.icloud",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("network timeout");
    });

    it("fetches plugin wasm from registry when no override and not builtin", async () => {
      const wasmBytes = new Uint8Array([10, 20, 30]);
      mocks.fetchPluginRegistry.mockResolvedValue({
        plugins: [
          { id: "custom.sync", name: "Custom Sync", artifact: { url: "https://example.com/plugin.wasm" } },
        ],
      });
      mocks.proxyFetch.mockResolvedValue({
        ok: true,
        arrayBuffer: () => Promise.resolve(wasmBytes.buffer),
      });
      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-2", filesImported: 5 });
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      mocks.getBackend.mockResolvedValue(restoredBackend);

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue(null),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await handleCreateWithProvider(
        deps as any,
        null,
        "custom.sync",
        null,
        { id: "remote-1", metadata: { name: "WS" } },
      );

      expect(mocks.proxyFetch).toHaveBeenCalledWith("https://example.com/plugin.wasm");
      expect(mocks.downloadWorkspace).toHaveBeenCalledWith(
        "custom.sync",
        expect.any(Object),
        undefined,
        undefined,
        expect.any(Uint8Array),
      );
    });

    it("throws when plugin not found in registry and no override", async () => {
      mocks.fetchPluginRegistry.mockResolvedValue({ plugins: [] });

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn(),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "unknown.plugin",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("Could not download the sync plugin");
    });

    it("throws when proxyFetch returns non-ok response", async () => {
      mocks.fetchPluginRegistry.mockResolvedValue({
        plugins: [
          { id: "custom.sync", artifact: { url: "https://example.com/plugin.wasm" } },
        ],
      });
      mocks.proxyFetch.mockResolvedValue({ ok: false });

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn(),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "custom.sync",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("Could not download the sync plugin");
    });

    it("falls back to metadata.provider when providerPluginId is null", async () => {
      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 1 });
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      mocks.getBackend.mockResolvedValue(restoredBackend);

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue(null),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await handleCreateWithProvider(
        deps as any,
        null,
        null,
        null,
        { id: "remote-1", metadata: { provider: "builtin.icloud", name: "WS" } },
      );

      expect(mocks.downloadWorkspace).toHaveBeenCalledWith(
        "builtin.icloud",
        expect.any(Object),
        undefined,
        undefined,
        null,
      );
    });

    it("defaults provider to diaryx.sync when no providerPluginId or metadata.provider", async () => {
      mocks.fetchPluginRegistry.mockResolvedValue({
        plugins: [
          { id: "diaryx.sync", artifact: { url: "https://example.com/sync.wasm" } },
        ],
      });
      const wasmBytes = new Uint8Array([1, 2]);
      mocks.proxyFetch.mockResolvedValue({
        ok: true,
        arrayBuffer: () => Promise.resolve(wasmBytes.buffer),
      });
      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 1 });
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      mocks.getBackend.mockResolvedValue(restoredBackend);

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue(null),
        getFrontmatter: vi.fn(),
      };
      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => api),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await handleCreateWithProvider(
        deps as any,
        null,
        null,
        null,
        { id: "remote-1", metadata: null },
      );

      expect(mocks.downloadWorkspace).toHaveBeenCalledWith(
        "diaryx.sync",
        expect.objectContaining({ name: "Restored Workspace" }),
        undefined,
        undefined,
        expect.any(Uint8Array),
      );
    });

    it("skips overridden plugins during registry inference", async () => {
      const providerOverrideBytes = new Uint8Array([1]).buffer;
      const pluginOverrideBytes = new Uint8Array([2]).buffer;
      const frontmatter = {
        plugins: { "diaryx.daily": { enabled: true }, "custom.overridden": {} },
      };
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("/ws/README.md"),
        getFrontmatter: vi.fn().mockResolvedValue(frontmatter),
      };

      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 3 });
      mocks.getBackend.mockResolvedValue(restoredBackend);
      mocks.fetchPluginRegistry.mockResolvedValue({
        plugins: [
          { id: "diaryx.daily", name: "Daily" },
          { id: "custom.overridden", name: "Overridden" },
        ],
      });

      const deps = {
        autoCreateDeps: {
          createApi: vi.fn(() => api),
          initEventSubscription: vi.fn(() => () => {}),
          persistPermissionDefaults: vi.fn(),
          setBackend: vi.fn(),
          setCleanupEventSubscription: vi.fn(),
        },
        dismissLaunchOverlay: vi.fn(),
        expandNode: vi.fn(),
        getTree: vi.fn(() => null),
        installLocalPlugin: vi.fn().mockResolvedValue(undefined),
        openEntry: vi.fn().mockResolvedValue(undefined),
        persistPermissionDefaults: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
        switchWorkspace: vi.fn(),
      };

      await handleCreateWithProvider(
        deps as any,
        null,
        "builtin.icloud",
        [
          { targetPluginId: "builtin.icloud", bytes: providerOverrideBytes, fileName: "icloud.wasm" },
          { targetPluginId: "custom.overridden", bytes: pluginOverrideBytes, fileName: "custom.wasm" },
        ],
        { id: "remote-1", metadata: { name: "WS" } },
      );

      // custom.overridden should be skipped by installRegistryPlugin
      expect(mocks.installRegistryPlugin).toHaveBeenCalledTimes(1);
      expect(mocks.installRegistryPlugin).toHaveBeenCalledWith({ id: "diaryx.daily", name: "Daily" });
    });

    it("tolerates plugin inference failure on restore (non-fatal)", async () => {
      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 1 });
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      mocks.getBackend.mockResolvedValue(restoredBackend);

      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockRejectedValue(new Error("resolve error")),
        getFrontmatter: vi.fn(),
      };

      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const deps = {
        autoCreateDeps: {
          createApi: vi.fn(() => api),
          initEventSubscription: vi.fn(() => () => {}),
          persistPermissionDefaults: vi.fn(),
          setBackend: vi.fn(),
          setCleanupEventSubscription: vi.fn(),
        },
        dismissLaunchOverlay: vi.fn(),
        expandNode: vi.fn(),
        getTree: vi.fn(() => null),
        installLocalPlugin: vi.fn().mockResolvedValue(undefined),
        openEntry: vi.fn().mockResolvedValue(undefined),
        persistPermissionDefaults: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
        switchWorkspace: vi.fn(),
      };

      // Should not throw
      await handleCreateWithProvider(
        deps as any,
        null,
        "builtin.icloud",
        null,
        { id: "remote-1", metadata: { name: "WS" } },
      );

      expect(warnSpy).toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("installs overrides in new workspace path (no restoreNamespace)", async () => {
      const { deps } = makeCreateWithProviderDeps();
      const overrideBytes = new ArrayBuffer(8);

      await handleCreateWithProvider(
        deps as any,
        { plugins: [{ plugin_id: "p1" }, { plugin_id: "p2" }] } as any,
        "my-sync",
        [{ targetPluginId: "p1", bytes: overrideBytes, fileName: "p1.wasm" }],
        null,
      );

      expect(deps.installLocalPlugin).toHaveBeenCalledWith(overrideBytes, "p1");
    });

    it("warns when restored workspace references unknown plugin", async () => {
      const frontmatter = { plugins: { "unknown.plugin": {} } };
      const restoredBackend = { getWorkspacePath: vi.fn(() => "/ws/README.md") };
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("/ws/README.md"),
        getFrontmatter: vi.fn().mockResolvedValue(frontmatter),
      };

      mocks.downloadWorkspace.mockResolvedValue({ localId: "ws-1", filesImported: 1 });
      mocks.getBackend.mockResolvedValue(restoredBackend);
      mocks.fetchPluginRegistry.mockResolvedValue({ plugins: [] });

      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const deps = {
        autoCreateDeps: {
          createApi: vi.fn(() => api),
          initEventSubscription: vi.fn(() => () => {}),
          persistPermissionDefaults: vi.fn(),
          setBackend: vi.fn(),
          setCleanupEventSubscription: vi.fn(),
        },
        dismissLaunchOverlay: vi.fn(),
        expandNode: vi.fn(),
        getTree: vi.fn(() => null),
        installLocalPlugin: vi.fn().mockResolvedValue(undefined),
        openEntry: vi.fn().mockResolvedValue(undefined),
        persistPermissionDefaults: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
        switchWorkspace: vi.fn(),
      };

      await handleCreateWithProvider(
        deps as any,
        null,
        "builtin.icloud",
        null,
        { id: "remote-1", metadata: { name: "WS" } },
      );

      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringContaining("unknown.plugin"),
      );
      expect(mocks.installRegistryPlugin).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("handles 'not implemented' download error as unsupported", async () => {
      mocks.downloadWorkspace.mockRejectedValue(new Error("not implemented"));

      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => ({
              resolveWorkspaceRootIndexPath: vi.fn(),
              getFrontmatter: vi.fn(),
            })),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "builtin.icloud",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("does not support downloading workspaces");
    });

    it("handles 'Unknown command' download error as unsupported", async () => {
      mocks.downloadWorkspace.mockRejectedValue(new Error("Unknown command download_workspace"));

      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => ({
              resolveWorkspaceRootIndexPath: vi.fn(),
              getFrontmatter: vi.fn(),
            })),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "builtin.icloud",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("does not support downloading workspaces");
    });

    it("handles 'No plugin X handles command' download error as unsupported", async () => {
      mocks.downloadWorkspace.mockRejectedValue(new Error("No plugin foo handles command bar"));

      const { deps } = makeCreateWithProviderDeps({
        providerDeps: {
          autoCreateDeps: {
            createApi: vi.fn(() => ({
              resolveWorkspaceRootIndexPath: vi.fn(),
              getFrontmatter: vi.fn(),
            })),
            initEventSubscription: vi.fn(() => () => {}),
            persistPermissionDefaults: vi.fn(),
            setBackend: vi.fn(),
            setCleanupEventSubscription: vi.fn(),
          },
        },
      });

      await expect(
        handleCreateWithProvider(
          deps as any,
          null,
          "builtin.icloud",
          null,
          { id: "remote-1", metadata: { name: "WS" } },
        ),
      ).rejects.toThrow("does not support downloading workspaces");
    });
  });

  // -------------------------------------------------------------------------
  // handleWelcomeComplete
  // -------------------------------------------------------------------------

  describe("handleWelcomeComplete", () => {
    it("refreshes backend and opens root entry", async () => {
      const backend = { name: "test-backend" };
      const deps = {
        getBackend: vi.fn().mockResolvedValue(backend),
        setBackend: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        getTree: vi.fn(() => ({ path: "/ws/index.md" })),
        getCurrentEntry: vi.fn(() => null),
        expandNode: vi.fn(),
        openEntry: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
      };

      await handleWelcomeComplete(deps, "ws-1", "My Workspace");

      expect(deps.getBackend).toHaveBeenCalled();
      expect(deps.setBackend).toHaveBeenCalledWith(backend);
      expect(deps.refreshTree).toHaveBeenCalled();
      expect(deps.expandNode).toHaveBeenCalledWith("/ws/index.md");
      expect(deps.openEntry).toHaveBeenCalledWith("/ws/index.md");
      expect(deps.runValidation).toHaveBeenCalled();
    });

    it("skips opening entry when currentEntry already exists", async () => {
      const deps = {
        getBackend: vi.fn().mockResolvedValue({}),
        setBackend: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        getTree: vi.fn(() => ({ path: "/ws/index.md" })),
        getCurrentEntry: vi.fn(() => ({ path: "/ws/existing.md" })),
        expandNode: vi.fn(),
        openEntry: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
      };

      await handleWelcomeComplete(deps, "ws-1", "My Workspace");

      expect(deps.expandNode).not.toHaveBeenCalled();
      expect(deps.openEntry).not.toHaveBeenCalled();
    });

    it("skips navigation when getTree returns null", async () => {
      const deps = {
        getBackend: vi.fn().mockResolvedValue({}),
        setBackend: vi.fn(),
        refreshTree: vi.fn().mockResolvedValue(undefined),
        getTree: vi.fn(() => null),
        getCurrentEntry: vi.fn(() => null),
        expandNode: vi.fn(),
        openEntry: vi.fn().mockResolvedValue(undefined),
        runValidation: vi.fn().mockResolvedValue(undefined),
      };

      await handleWelcomeComplete(deps, "ws-1", "My Workspace");

      expect(deps.expandNode).not.toHaveBeenCalled();
      expect(deps.openEntry).not.toHaveBeenCalled();
      expect(deps.runValidation).toHaveBeenCalled();
    });
  });
});
