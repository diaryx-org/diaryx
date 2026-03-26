import { describe, it, expect, beforeEach, vi } from "vitest";

// ---------------------------------------------------------------------------
// Mocks – must use vi.hoisted since vi.mock is hoisted above declarations
// ---------------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  getBrowserManifests: vi.fn(() => [] as any[]),
  getCachedPluginIcon: vi.fn(() => ({}) as any),
  loadPluginIcon: vi.fn(async () => ({}) as any),
}));

vi.mock("$lib/plugins/browserPluginManager.svelte", () => ({
  getBrowserManifests: mocks.getBrowserManifests,
  dispatchCommand: vi.fn(),
  getPlugin: vi.fn(() => null),
}));

vi.mock("$lib/plugins/pluginIconResolver", () => ({
  getCachedPluginIcon: mocks.getCachedPluginIcon,
  loadPluginIcon: mocks.loadPluginIcon,
}));

import { getPluginStore } from "./pluginStore.svelte";
import type { PluginManifest } from "$lib/backend/generated";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeManifest(
  overrides: Partial<PluginManifest> & { id: string },
): PluginManifest {
  return {
    name: overrides.id,
    version: "0.1.0",
    description: null,
    ui: [],
    capabilities: [],
    ...overrides,
  } as unknown as PluginManifest;
}

function makeMockApi(manifests: PluginManifest[] = []) {
  return {
    getPluginManifests: vi.fn(async () => manifests),
  } as any;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("pluginStore", () => {
  let store: ReturnType<typeof getPluginStore>;

  beforeEach(async () => {
    vi.clearAllMocks();
    localStorage.clear();
    mocks.getBrowserManifests.mockReturnValue([]);
    store = getPluginStore();
    // Reset all module-level state: init reads pluginEnabledState from
    // (now-empty) localStorage and resets backendManifests to [].
    await store.init({ getPluginManifests: async () => [] } as any);
    // Wire up persist callback (no-op) so setPluginEnabled can call it
    store.hydrateDisabledPlugins([], vi.fn(async () => {}));
    // Clear any runtime overrides
    store.clearRuntimeManifestOverrides();
  });

  // ========================================================================
  // isPluginEnabled / setPluginEnabled
  // ========================================================================

  describe("isPluginEnabled", () => {
    it("returns true by default for unknown plugins", () => {
      expect(store.isPluginEnabled("some.unknown.plugin")).toBe(true);
    });

    it("returns false when plugin has been explicitly disabled", () => {
      store.setPluginEnabled("my.plugin", false);
      expect(store.isPluginEnabled("my.plugin")).toBe(false);
    });

    it("returns true when plugin has been explicitly enabled", () => {
      store.setPluginEnabled("my.plugin", false);
      store.setPluginEnabled("my.plugin", true);
      expect(store.isPluginEnabled("my.plugin")).toBe(true);
    });
  });

  describe("setPluginEnabled", () => {
    it("persists disabled state to localStorage", () => {
      store.setPluginEnabled("test.plugin", false);
      const stored = JSON.parse(
        localStorage.getItem("diaryx-plugin-enabled") ?? "{}",
      );
      expect(stored["test.plugin"]).toBe(false);
    });

    it("persists enabled state to localStorage", () => {
      store.setPluginEnabled("test.plugin", false);
      store.setPluginEnabled("test.plugin", true);
      const stored = JSON.parse(
        localStorage.getItem("diaryx-plugin-enabled") ?? "{}",
      );
      expect(stored["test.plugin"]).toBe(true);
    });

    it("is a no-op when setting the same state", () => {
      // Default is true, setting to true should not call setItem
      store.setPluginEnabled("noop.plugin", true);
      // localStorage.setItem should not have been called for this
      expect(localStorage.setItem).not.toHaveBeenCalled();
    });

    it("calls persistDisabledPlugins callback with disabled ids", () => {
      const persistFn = vi.fn(async () => {});
      store.hydrateDisabledPlugins([], persistFn);

      store.setPluginEnabled("a", false);
      expect(persistFn).toHaveBeenCalledWith(["a"]);

      store.setPluginEnabled("b", false);
      expect(persistFn).toHaveBeenCalledWith(
        expect.arrayContaining(["a", "b"]),
      );
    });
  });

  describe("clearPluginEnabled", () => {
    it("removes the plugin entry from enabled state", () => {
      store.setPluginEnabled("clear.me", false);
      expect(store.isPluginEnabled("clear.me")).toBe(false);

      store.clearPluginEnabled("clear.me");
      // After clearing, it falls back to default (true)
      expect(store.isPluginEnabled("clear.me")).toBe(true);
    });

    it("is a no-op for plugins not in state", () => {
      // Should not throw
      store.clearPluginEnabled("nonexistent.plugin");
      expect(store.isPluginEnabled("nonexistent.plugin")).toBe(true);
    });
  });

  // ========================================================================
  // hydrateDisabledPlugins
  // ========================================================================

  describe("hydrateDisabledPlugins", () => {
    it("marks provided plugin ids as disabled", () => {
      store.hydrateDisabledPlugins(
        ["plugin.a", "plugin.b"],
        vi.fn(async () => {}),
      );
      expect(store.isPluginEnabled("plugin.a")).toBe(false);
      expect(store.isPluginEnabled("plugin.b")).toBe(false);
      // Other plugins still enabled by default
      expect(store.isPluginEnabled("plugin.c")).toBe(true);
    });

    it("handles undefined disabled list", () => {
      store.hydrateDisabledPlugins(undefined, vi.fn(async () => {}));
      // Everything should still be enabled
      expect(store.isPluginEnabled("any.plugin")).toBe(true);
    });

    it("handles empty disabled list", () => {
      store.hydrateDisabledPlugins([], vi.fn(async () => {}));
      expect(store.isPluginEnabled("any.plugin")).toBe(true);
    });
  });

  // ========================================================================
  // init
  // ========================================================================

  describe("init", () => {
    it("loads manifests from the API", async () => {
      const m = makeManifest({ id: "backend.plugin", ui: [] });
      const api = makeMockApi([m]);

      await store.init(api);
      expect(api.getPluginManifests).toHaveBeenCalledOnce();

      const ids = store.allManifests.map((x) => String(x.id));
      expect(ids).toContain("backend.plugin");
    });

    it("handles API failure gracefully", async () => {
      const api = {
        getPluginManifests: vi.fn(async () => {
          throw new Error("network error");
        }),
      } as any;

      const consoleSpy = vi
        .spyOn(console, "warn")
        .mockImplementation(() => {});
      await store.init(api);

      expect(store.allManifests).toEqual([]);
      consoleSpy.mockRestore();
    });

    it("loads plugin enabled state from localStorage on init", async () => {
      localStorage.setItem(
        "diaryx-plugin-enabled",
        JSON.stringify({ "stored.plugin": false }),
      );
      const api = makeMockApi([]);
      await store.init(api);

      expect(store.isPluginEnabled("stored.plugin")).toBe(false);
    });
  });

  // ========================================================================
  // Manifest management
  // ========================================================================

  describe("manifest management", () => {
    it("includes browser manifests in allManifests", async () => {
      const browserManifest = makeManifest({ id: "browser.plugin" });
      mocks.getBrowserManifests.mockReturnValue([browserManifest]);

      const api = makeMockApi([]);
      await store.init(api);

      const ids = store.allManifests.map((x) => String(x.id));
      expect(ids).toContain("browser.plugin");
    });

    it("runtime overrides take precedence over backend manifests", async () => {
      const backendManifest = makeManifest({
        id: "shared.plugin",
        name: "Backend Version",
      });
      const runtimeManifest = makeManifest({
        id: "shared.plugin",
        name: "Runtime Version",
      });

      const api = makeMockApi([backendManifest]);
      await store.init(api);
      store.setRuntimeManifestOverride(runtimeManifest);

      const found = store.allManifests.find(
        (m) => String(m.id) === "shared.plugin",
      );
      expect(found?.name).toBe("Runtime Version");
    });

    it("clearRuntimeManifestOverride removes a single override", async () => {
      const m = makeManifest({ id: "rm.plugin" });
      store.setRuntimeManifestOverride(m);
      expect(
        store.allManifests.some((x) => String(x.id) === "rm.plugin"),
      ).toBe(true);

      store.clearRuntimeManifestOverride("rm.plugin");
      expect(
        store.allManifests.some((x) => String(x.id) === "rm.plugin"),
      ).toBe(false);
    });

    it("clearRuntimeManifestOverrides removes all overrides", async () => {
      store.setRuntimeManifestOverride(makeManifest({ id: "a" }));
      store.setRuntimeManifestOverride(makeManifest({ id: "b" }));
      expect(store.allManifests.length).toBeGreaterThanOrEqual(2);

      store.clearRuntimeManifestOverrides();
      const ids = store.allManifests.map((x) => String(x.id));
      expect(ids).not.toContain("a");
      expect(ids).not.toContain("b");
    });
  });

  // ========================================================================
  // manifests (enabled-only filter)
  // ========================================================================

  describe("manifests (enabled filter)", () => {
    it("excludes disabled plugins from manifests", async () => {
      const api = makeMockApi([
        makeManifest({ id: "enabled.plugin" }),
        makeManifest({ id: "disabled.plugin" }),
      ]);
      await store.init(api);
      store.setPluginEnabled("disabled.plugin", false);

      const ids = store.manifests.map((m) => String(m.id));
      expect(ids).toContain("enabled.plugin");
      expect(ids).not.toContain("disabled.plugin");
    });

    it("includes all plugins when none are disabled", async () => {
      const api = makeMockApi([
        makeManifest({ id: "p1" }),
        makeManifest({ id: "p2" }),
      ]);
      await store.init(api);

      expect(store.manifests.length).toBe(2);
    });
  });

  // ========================================================================
  // UI contribution selectors
  // ========================================================================

  describe("UI contribution selectors", () => {
    it("returns settings tabs from enabled plugins", async () => {
      const manifest = makeManifest({
        id: "ui.plugin",
        ui: [
          {
            slot: "SettingsTab",
            label: "My Settings",
            icon: "settings",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const tabs = store.settingsTabs;
      expect(tabs).toHaveLength(1);
      expect(tabs[0].contribution.slot).toBe("SettingsTab");
    });

    it("returns left sidebar tabs", async () => {
      const manifest = makeManifest({
        id: "sidebar.plugin",
        ui: [
          {
            slot: "SidebarTab",
            side: "Left",
            label: "Left Tab",
            icon: "folder",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.leftSidebarTabs).toHaveLength(1);
      expect(store.rightSidebarTabs).toHaveLength(0);
    });

    it("returns right sidebar tabs", async () => {
      const manifest = makeManifest({
        id: "sidebar.plugin",
        ui: [
          {
            slot: "SidebarTab",
            side: "Right",
            label: "Right Tab",
            icon: "folder",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.rightSidebarTabs).toHaveLength(1);
      expect(store.leftSidebarTabs).toHaveLength(0);
    });

    it("returns toolbar buttons", async () => {
      const manifest = makeManifest({
        id: "toolbar.plugin",
        ui: [
          {
            slot: "ToolbarButton",
            label: "My Button",
            icon: "zap",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.toolbarButtons).toHaveLength(1);
    });

    it("returns status bar items", async () => {
      const manifest = makeManifest({
        id: "status.plugin",
        ui: [
          {
            slot: "StatusBarItem",
            label: "Status",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.statusBarItems).toHaveLength(1);
    });

    it("returns command palette items", async () => {
      const manifest = makeManifest({
        id: "cmd.plugin",
        ui: [
          {
            slot: "CommandPaletteItem",
            label: "Do Something",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.commandPaletteItems).toHaveLength(1);
    });

    it("returns command palette owner (first match)", async () => {
      const manifest = makeManifest({
        id: "palette.owner",
        ui: [{ slot: "CommandPalette" }] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const owner = store.commandPaletteOwner;
      expect(owner).not.toBeNull();
      expect(String(owner!.pluginId)).toBe("palette.owner");
    });

    it("returns null when no command palette owner", async () => {
      const api = makeMockApi([]);
      await store.init(api);
      expect(store.commandPaletteOwner).toBeNull();
    });

    it("returns left sidebar context menu owner", async () => {
      const manifest = makeManifest({
        id: "ctx.plugin",
        ui: [
          { slot: "ContextMenu", target: "LeftSidebarTree" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const owner = store.leftSidebarContextMenuOwner;
      expect(owner).not.toBeNull();
      expect(String(owner!.pluginId)).toBe("ctx.plugin");
    });

    it("returns null when no context menu owner", async () => {
      const api = makeMockApi([]);
      await store.init(api);
      expect(store.leftSidebarContextMenuOwner).toBeNull();
    });

    it("does not include contributions from disabled plugins", async () => {
      const manifest = makeManifest({
        id: "disabled.ui.plugin",
        ui: [
          { slot: "SettingsTab", label: "Hidden", icon: "x" },
          { slot: "ToolbarButton", label: "Hidden", icon: "x" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);
      store.setPluginEnabled("disabled.ui.plugin", false);

      expect(store.settingsTabs).toHaveLength(0);
      expect(store.toolbarButtons).toHaveLength(0);
    });
  });

  // ========================================================================
  // Editor insert commands
  // ========================================================================

  describe("editorInsertCommands", () => {
    it("categorizes inline, block, and mark commands", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "inline_ext",
            node_type: "InlineAtom",
            insert_command: { label: "Inline", icon: null },
          },
          {
            slot: "EditorExtension",
            extension_id: "block_ext",
            node_type: "BlockAtom",
            insert_command: { label: "Block", icon: null },
          },
          {
            slot: "EditorExtension",
            extension_id: "mark_ext",
            node_type: "InlineMark",
            insert_command: { label: "Mark", icon: null },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const cmds = store.editorInsertCommands;
      expect(cmds.inline).toHaveLength(1);
      expect(cmds.block).toHaveLength(1);
      expect(cmds.mark).toHaveLength(1);
      expect(cmds.inline[0].extensionId).toBe("inline_ext");
      expect(cmds.block[0].extensionId).toBe("block_ext");
      expect(cmds.mark[0].extensionId).toBe("mark_ext");
    });

    it("skips editor extensions without insert_command", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "no_cmd",
            node_type: "InlineAtom",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const cmds = store.editorInsertCommands;
      expect(cmds.inline).toHaveLength(0);
      expect(cmds.block).toHaveLength(0);
      expect(cmds.mark).toHaveLength(0);
    });

    it("defaults placement to Picker", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext",
            node_type: "InlineAtom",
            insert_command: { label: "Cmd", icon: null },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.editorInsertCommands.inline[0].placement).toBe("Picker");
    });
  });

  // ========================================================================
  // Workspace providers
  // ========================================================================

  describe("workspaceProviders", () => {
    it("synthesizes provider from custom commands capability", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "Workspace Provider",
        capabilities: [
          {
            CustomCommands: {
              commands: [
                "GetProviderStatus",
                "ListRemoteWorkspaces",
                "LinkWorkspace",
                "UnlinkWorkspace",
                "DownloadWorkspace",
              ],
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.workspaceProviders;
      expect(providers).toHaveLength(1);
      expect(providers[0].contribution.label).toBe("Workspace Provider");
    });
  });
});
