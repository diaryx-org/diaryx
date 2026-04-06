import { describe, it, expect, beforeEach, vi } from "vitest";

// ---------------------------------------------------------------------------
// Mocks – must use vi.hoisted since vi.mock is hoisted above declarations
// ---------------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  getBrowserManifests: vi.fn(() => [] as any[]),
  getCachedPluginIcon: vi.fn(() => ({}) as any),
  loadPluginIcon: vi.fn(async () => ({}) as any),
  getBuiltinWorkspaceProviders: vi.fn(() => [] as any[]),
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

vi.mock("$lib/sync/builtinProviders", () => ({
  getBuiltinWorkspaceProviders: mocks.getBuiltinWorkspaceProviders,
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

/** All five commands required for workspace provider synthesis. */
const ALL_WP_COMMANDS = [
  "GetProviderStatus",
  "ListRemoteWorkspaces",
  "LinkWorkspace",
  "UnlinkWorkspace",
  "DownloadWorkspace",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("pluginStore", () => {
  let store: ReturnType<typeof getPluginStore>;

  beforeEach(async () => {
    vi.clearAllMocks();
    localStorage.clear();
    mocks.getBrowserManifests.mockReturnValue([]);
    mocks.getBuiltinWorkspaceProviders.mockReturnValue([]);
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

    it("is a no-op when disabling an already-disabled plugin", () => {
      store.setPluginEnabled("x", false);
      vi.clearAllMocks();
      store.setPluginEnabled("x", false);
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

    it("only includes disabled ids in the persist callback", () => {
      const persistFn = vi.fn(async () => {});
      store.hydrateDisabledPlugins([], persistFn);

      store.setPluginEnabled("a", false);
      store.setPluginEnabled("b", false);
      store.setPluginEnabled("a", true);

      // Last call should only have "b" as disabled
      const lastCall = persistFn.mock.calls.at(-1) as unknown[];
      expect(lastCall[0]).toEqual(["b"]);
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

    it("persists updated state after clearing", () => {
      const persistFn = vi.fn(async () => {});
      store.hydrateDisabledPlugins([], persistFn);

      store.setPluginEnabled("clear.me", false);
      store.clearPluginEnabled("clear.me");

      // The last persist call should not include "clear.me" as disabled
      const lastCall = persistFn.mock.calls.at(-1) as unknown[];
      expect(lastCall[0]).not.toContain("clear.me");
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

    it("replaces previous disabled state entirely", () => {
      store.setPluginEnabled("old.plugin", false);
      expect(store.isPluginEnabled("old.plugin")).toBe(false);

      // Hydrate with a different set of disabled plugins
      store.hydrateDisabledPlugins(["new.plugin"], vi.fn(async () => {}));
      // old.plugin is no longer tracked as disabled because hydrate replaces state
      expect(store.isPluginEnabled("old.plugin")).toBe(true);
      expect(store.isPluginEnabled("new.plugin")).toBe(false);
    });

    it("wires up the persist callback for future setPluginEnabled calls", () => {
      const persistFn = vi.fn(async () => {});
      store.hydrateDisabledPlugins([], persistFn);

      store.setPluginEnabled("x", false);
      expect(persistFn).toHaveBeenCalled();
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

    it("handles corrupt JSON in localStorage gracefully", async () => {
      localStorage.setItem("diaryx-plugin-enabled", "not-valid-json{{{");
      const api = makeMockApi([]);
      await store.init(api);

      // Should fall back to defaults (all enabled)
      expect(store.isPluginEnabled("any.plugin")).toBe(true);
    });

    it("handles non-object JSON in localStorage gracefully", async () => {
      localStorage.setItem("diaryx-plugin-enabled", '"just a string"');
      const api = makeMockApi([]);
      await store.init(api);

      expect(store.isPluginEnabled("any.plugin")).toBe(true);
    });

    it("handles null JSON in localStorage gracefully", async () => {
      localStorage.setItem("diaryx-plugin-enabled", "null");
      const api = makeMockApi([]);
      await store.init(api);

      expect(store.isPluginEnabled("any.plugin")).toBe(true);
    });

    it("replaces previous backend manifests on re-init", async () => {
      const api1 = makeMockApi([makeManifest({ id: "first" })]);
      await store.init(api1);
      expect(store.allManifests.map((m) => String(m.id))).toContain("first");

      const api2 = makeMockApi([makeManifest({ id: "second" })]);
      await store.init(api2);
      const ids = store.allManifests.map((m) => String(m.id));
      expect(ids).toContain("second");
      expect(ids).not.toContain("first");
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

    it("browser manifests override backend manifests with same id", async () => {
      const backendManifest = makeManifest({
        id: "shared.id",
        name: "Backend",
      });
      const browserManifest = makeManifest({
        id: "shared.id",
        name: "Browser",
      });
      mocks.getBrowserManifests.mockReturnValue([browserManifest]);

      const api = makeMockApi([backendManifest]);
      await store.init(api);

      const found = store.allManifests.find(
        (m) => String(m.id) === "shared.id",
      );
      expect(found?.name).toBe("Browser");
      // Should only appear once
      expect(
        store.allManifests.filter((m) => String(m.id) === "shared.id"),
      ).toHaveLength(1);
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

    it("runtime overrides take precedence over browser manifests", async () => {
      const browserManifest = makeManifest({
        id: "shared.plugin",
        name: "Browser Version",
      });
      mocks.getBrowserManifests.mockReturnValue([browserManifest]);

      const runtimeManifest = makeManifest({
        id: "shared.plugin",
        name: "Runtime Version",
      });

      const api = makeMockApi([]);
      await store.init(api);
      store.setRuntimeManifestOverride(runtimeManifest);

      const found = store.allManifests.find(
        (m) => String(m.id) === "shared.plugin",
      );
      expect(found?.name).toBe("Runtime Version");
    });

    it("setRuntimeManifestOverride updates existing override", async () => {
      store.setRuntimeManifestOverride(
        makeManifest({ id: "x", name: "V1" }),
      );
      store.setRuntimeManifestOverride(
        makeManifest({ id: "x", name: "V2" }),
      );

      const found = store.allManifests.find(
        (m) => String(m.id) === "x",
      );
      expect(found?.name).toBe("V2");
      expect(
        store.allManifests.filter((m) => String(m.id) === "x"),
      ).toHaveLength(1);
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

    it("clearRuntimeManifestOverride is a no-op for non-existent id", () => {
      // Should not throw
      store.clearRuntimeManifestOverride("does.not.exist");
      expect(store.allManifests).toEqual([]);
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

    it("merges manifests from all three sources without duplicates", async () => {
      const backendM = makeManifest({ id: "backend-only" });
      const browserM = makeManifest({ id: "browser-only" });
      const runtimeM = makeManifest({ id: "runtime-only" });
      mocks.getBrowserManifests.mockReturnValue([browserM]);

      const api = makeMockApi([backendM]);
      await store.init(api);
      store.setRuntimeManifestOverride(runtimeM);

      const ids = store.allManifests.map((m) => String(m.id));
      expect(ids).toContain("backend-only");
      expect(ids).toContain("browser-only");
      expect(ids).toContain("runtime-only");
      expect(store.allManifests).toHaveLength(3);
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

    it("re-enables plugin in manifests after setPluginEnabled(true)", async () => {
      const api = makeMockApi([makeManifest({ id: "toggle.plugin" })]);
      await store.init(api);

      store.setPluginEnabled("toggle.plugin", false);
      expect(store.manifests.map((m) => String(m.id))).not.toContain(
        "toggle.plugin",
      );

      store.setPluginEnabled("toggle.plugin", true);
      expect(store.manifests.map((m) => String(m.id))).toContain(
        "toggle.plugin",
      );
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

    it("returns multiple settings tabs from multiple plugins", async () => {
      const m1 = makeManifest({
        id: "plugin.a",
        ui: [{ slot: "SettingsTab", label: "Tab A", icon: "a" }] as any,
      });
      const m2 = makeManifest({
        id: "plugin.b",
        ui: [{ slot: "SettingsTab", label: "Tab B", icon: "b" }] as any,
      });
      const api = makeMockApi([m1, m2]);
      await store.init(api);

      const tabs = store.settingsTabs;
      expect(tabs).toHaveLength(2);
      expect(tabs.map((t) => t.contribution.label)).toEqual(
        expect.arrayContaining(["Tab A", "Tab B"]),
      );
    });

    it("returns settings tabs with correct pluginId", async () => {
      const manifest = makeManifest({
        id: "my.plugin",
        ui: [{ slot: "SettingsTab", label: "Tab", icon: "x" }] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(String(store.settingsTabs[0].pluginId)).toBe("my.plugin");
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

    it("separates left and right sidebar tabs from same plugin", async () => {
      const manifest = makeManifest({
        id: "sidebar.plugin",
        ui: [
          { slot: "SidebarTab", side: "Left", label: "L", icon: "a" },
          { slot: "SidebarTab", side: "Right", label: "R", icon: "b" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.leftSidebarTabs).toHaveLength(1);
      expect(store.rightSidebarTabs).toHaveLength(1);
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

    it("returns first command palette owner when multiple exist", async () => {
      const m1 = makeManifest({
        id: "first.palette",
        ui: [{ slot: "CommandPalette" }] as any,
      });
      const m2 = makeManifest({
        id: "second.palette",
        ui: [{ slot: "CommandPalette" }] as any,
      });
      const api = makeMockApi([m1, m2]);
      await store.init(api);

      const owner = store.commandPaletteOwner;
      expect(owner).not.toBeNull();
      expect(String(owner!.pluginId)).toBe("first.palette");
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

    it("ignores context menu with different target", async () => {
      const manifest = makeManifest({
        id: "ctx.plugin",
        ui: [
          { slot: "ContextMenu", target: "SomeOtherTarget" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.leftSidebarContextMenuOwner).toBeNull();
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
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);
      store.setPluginEnabled("disabled.ui.plugin", false);

      expect(store.settingsTabs).toHaveLength(0);
    });

    it("returns empty arrays when no plugins have contributions", async () => {
      const manifest = makeManifest({ id: "empty.plugin", ui: [] });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.settingsTabs).toHaveLength(0);
      expect(store.leftSidebarTabs).toHaveLength(0);
      expect(store.rightSidebarTabs).toHaveLength(0);
      expect(store.commandPaletteItems).toHaveLength(0);
      expect(store.statusBarItems).toHaveLength(0);
    });

    it("filters contributions by slot type correctly with mixed UI entries", async () => {
      const manifest = makeManifest({
        id: "mixed.plugin",
        ui: [
          { slot: "SettingsTab", label: "Settings", icon: "s" },
          { slot: "StatusBarItem", label: "Status" },
          { slot: "CommandPaletteItem", label: "Cmd" },
          { slot: "SidebarTab", side: "Left", label: "Left", icon: "l" },
          { slot: "SidebarTab", side: "Right", label: "Right", icon: "r" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.settingsTabs).toHaveLength(1);
      expect(store.statusBarItems).toHaveLength(1);
      expect(store.commandPaletteItems).toHaveLength(1);
      expect(store.leftSidebarTabs).toHaveLength(1);
      expect(store.rightSidebarTabs).toHaveLength(1);
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

    it("uses explicit placement when provided", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext",
            node_type: "BlockAtom",
            insert_command: {
              label: "Cmd",
              icon: null,
              placement: "PickerAndStylePicker",
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.editorInsertCommands.block[0].placement).toBe(
        "PickerAndStylePicker",
      );
    });

    it("populates description and iconName from insert_command", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext",
            node_type: "InlineAtom",
            insert_command: {
              label: "My Cmd",
              icon: "star",
              description: "Insert a star",
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const cmd = store.editorInsertCommands.inline[0];
      expect(cmd.label).toBe("My Cmd");
      expect(cmd.iconName).toBe("star");
      expect(cmd.description).toBe("Insert a star");
    });

    it("defaults description to null when not provided", async () => {
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

      expect(store.editorInsertCommands.inline[0].description).toBeNull();
    });

    it("calls getCachedPluginIcon for the icon name", async () => {
      const fakeIcon = { name: "FakeStar" };
      mocks.getCachedPluginIcon.mockReturnValue(fakeIcon);

      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext",
            node_type: "InlineAtom",
            insert_command: { label: "Cmd", icon: "star" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const cmd = store.editorInsertCommands.inline[0];
      expect(cmd.icon).toBe(fakeIcon);
      expect(mocks.getCachedPluginIcon).toHaveBeenCalledWith("star");
    });

    it("skips non-EditorExtension slots", async () => {
      const manifest = makeManifest({
        id: "editor.plugin",
        ui: [
          { slot: "SettingsTab", label: "Settings", icon: "s" },
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

      const cmds = store.editorInsertCommands;
      expect(cmds.inline).toHaveLength(1);
    });

    it("collects commands from multiple plugins", async () => {
      const m1 = makeManifest({
        id: "plugin.a",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext_a",
            node_type: "InlineAtom",
            insert_command: { label: "A", icon: null },
          },
        ] as any,
      });
      const m2 = makeManifest({
        id: "plugin.b",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext_b",
            node_type: "InlineAtom",
            insert_command: { label: "B", icon: null },
          },
        ] as any,
      });
      const api = makeMockApi([m1, m2]);
      await store.init(api);

      expect(store.editorInsertCommands.inline).toHaveLength(2);
    });
  });

  // ========================================================================
  // Mark toolbar entries
  // ========================================================================

  describe("markToolbarEntries", () => {
    it("returns mark entries with toolbar config", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "highlight",
            node_type: "InlineMark",
            toolbar: { icon: "highlighter", label: "Highlight" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const entries = store.markToolbarEntries;
      expect(entries).toHaveLength(1);
      expect(entries[0].extensionId).toBe("highlight");
      expect(entries[0].label).toBe("Highlight");
      expect(entries[0].iconName).toBe("highlighter");
    });

    it("returns null attribute when no valid_values in attributes", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "bold",
            node_type: "InlineMark",
            toolbar: { icon: "bold", label: "Bold" },
            attributes: [{ name: "level", default: "1" }],
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.markToolbarEntries[0].attribute).toBeNull();
    });

    it("populates attribute from first attribute with valid_values", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "color",
            node_type: "InlineMark",
            toolbar: { icon: "palette", label: "Color" },
            attributes: [
              { name: "novals" },
              {
                name: "color",
                default: "red",
                valid_values: ["red", "blue", "green"],
                css_class_prefix: "text-",
              },
            ],
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const entry = store.markToolbarEntries[0];
      expect(entry.attribute).not.toBeNull();
      expect(entry.attribute!.name).toBe("color");
      expect(entry.attribute!.default).toBe("red");
      expect(entry.attribute!.validValues).toEqual(["red", "blue", "green"]);
      expect(entry.attribute!.cssClassPrefix).toBe("text-");
    });

    it("defaults cssClassPrefix to null when not provided", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "color",
            node_type: "InlineMark",
            toolbar: { icon: "palette", label: "Color" },
            attributes: [
              {
                name: "color",
                default: "red",
                valid_values: ["red", "blue"],
              },
            ],
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.markToolbarEntries[0].attribute!.cssClassPrefix).toBeNull();
    });

    it("skips marks without toolbar config", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "hidden-mark",
            node_type: "InlineMark",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.markToolbarEntries).toHaveLength(0);
    });

    it("skips non-InlineMark node types with toolbar", async () => {
      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "block-thing",
            node_type: "BlockAtom",
            toolbar: { icon: "box", label: "Block" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.markToolbarEntries).toHaveLength(0);
    });

    it("calls getCachedPluginIcon for toolbar icon", async () => {
      const fakeIcon = { name: "FakeHighlighter" };
      mocks.getCachedPluginIcon.mockReturnValue(fakeIcon);

      const manifest = makeManifest({
        id: "mark.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "highlight",
            node_type: "InlineMark",
            toolbar: { icon: "highlighter", label: "Highlight" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.markToolbarEntries[0].icon).toBe(fakeIcon);
      expect(mocks.getCachedPluginIcon).toHaveBeenCalledWith("highlighter");
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
              commands: ALL_WP_COMMANDS,
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.workspaceProviders;
      expect(providers).toHaveLength(1);
      expect(providers[0].contribution.label).toBe("Workspace Provider");
      expect(providers[0].source).toBe("plugin");
    });

    it("does not synthesize provider when commands are incomplete", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "Incomplete Provider",
        capabilities: [
          {
            CustomCommands: {
              commands: ["GetProviderStatus", "ListRemoteWorkspaces"],
              // Missing LinkWorkspace, UnlinkWorkspace, DownloadWorkspace
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      // Should only have builtin providers (mocked to empty)
      expect(store.workspaceProviders).toHaveLength(0);
    });

    it("uses explicit WorkspaceProvider UI entry over command synthesis", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "Plugin Name",
        ui: [
          {
            slot: "WorkspaceProvider",
            id: "custom-wp",
            label: "Custom WP Label",
            description: "A custom workspace provider",
          },
        ] as any,
        capabilities: [
          { CustomCommands: { commands: ALL_WP_COMMANDS } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.workspaceProviders;
      // Should use the explicit UI entry, not synthesize from commands
      expect(providers).toHaveLength(1);
      expect(providers[0].contribution.id).toBe("custom-wp");
      expect(providers[0].contribution.label).toBe("Custom WP Label");
      expect(providers[0].contribution.description).toBe(
        "A custom workspace provider",
      );
    });

    it("falls back to plugin id when WorkspaceProvider UI entry has no id", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "My WP",
        ui: [
          {
            slot: "WorkspaceProvider",
            label: "WP Label",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.workspaceProviders;
      expect(providers[0].contribution.id).toBe("wp.plugin");
    });

    it("falls back to manifest name when WorkspaceProvider has no label", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "My WP Name",
        ui: [
          {
            slot: "WorkspaceProvider",
            id: "wp-id",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.workspaceProviders[0].contribution.label).toBe("My WP Name");
    });

    it("falls back to id when manifest name is missing for synthesized provider", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: null as any,
        capabilities: [
          { CustomCommands: { commands: ALL_WP_COMMANDS } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.workspaceProviders[0].contribution.label).toBe("wp.plugin");
    });

    it("includes description for synthesized providers", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        name: "WP",
        description: "Syncs via magic",
        capabilities: [
          { CustomCommands: { commands: ALL_WP_COMMANDS } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.workspaceProviders[0].contribution.description).toBe(
        "Syncs via magic",
      );
    });

    it("appends builtin workspace providers", async () => {
      const builtinProvider = {
        pluginId: "builtin.icloud",
        contribution: {
          id: "icloud",
          label: "iCloud",
          description: null,
        },
        source: "builtin",
      };
      mocks.getBuiltinWorkspaceProviders.mockReturnValue([builtinProvider]);

      const api = makeMockApi([]);
      await store.init(api);

      const providers = store.workspaceProviders;
      expect(providers).toHaveLength(1);
      expect(providers[0].contribution.id).toBe("icloud");
    });

    it("combines plugin and builtin workspace providers", async () => {
      const builtinProvider = {
        pluginId: "builtin.icloud",
        contribution: { id: "icloud", label: "iCloud", description: null },
        source: "builtin",
      };
      mocks.getBuiltinWorkspaceProviders.mockReturnValue([builtinProvider]);

      const manifest = makeManifest({
        id: "wp.plugin",
        name: "Custom WP",
        capabilities: [
          { CustomCommands: { commands: ALL_WP_COMMANDS } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.workspaceProviders;
      expect(providers).toHaveLength(2);
      const ids = providers.map((p) => p.contribution.id);
      expect(ids).toContain("wp.plugin");
      expect(ids).toContain("icloud");
    });

    it("handles capabilities that are not CustomCommands", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        capabilities: [
          { SomeOtherCapability: {} },
          null,
          "string-cap",
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      // Should not produce any provider
      expect(store.workspaceProviders).toHaveLength(0);
    });

    it("handles empty commands array in CustomCommands", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        capabilities: [
          { CustomCommands: { commands: [] } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.workspaceProviders).toHaveLength(0);
    });

    it("ignores non-string commands in CustomCommands", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        capabilities: [
          { CustomCommands: { commands: [42, null, undefined, "GetProviderStatus"] } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      // Only "GetProviderStatus" is valid, but not enough to synthesize
      expect(store.workspaceProviders).toHaveLength(0);
    });

    it("ignores empty-string commands in CustomCommands", async () => {
      const manifest = makeManifest({
        id: "wp.plugin",
        capabilities: [
          { CustomCommands: { commands: ["", ...ALL_WP_COMMANDS] } },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      // Empty strings are skipped, but all required commands are present
      expect(store.workspaceProviders).toHaveLength(1);
    });
  });

  // ========================================================================
  // Storage providers
  // ========================================================================

  describe("storageProviders", () => {
    it("returns storage provider from legacy UI entry", async () => {
      const manifest = makeManifest({
        id: "storage.plugin",
        name: "My Storage",
        ui: [
          {
            slot: "StorageProvider",
            id: "sp-1",
            label: "My Storage Provider",
            description: "Stores stuff",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const providers = store.storageProviders;
      expect(providers).toHaveLength(1);
      expect(providers[0].contribution.id).toBe("sp-1");
      expect(providers[0].contribution.label).toBe("My Storage Provider");
      expect(providers[0].contribution.description).toBe("Stores stuff");
      expect(String(providers[0].pluginId)).toBe("storage.plugin");
    });

    it("falls back to plugin id when StorageProvider has no id", async () => {
      const manifest = makeManifest({
        id: "storage.plugin",
        name: "Storage",
        ui: [
          { slot: "StorageProvider", label: "Label" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.storageProviders[0].contribution.id).toBe("storage.plugin");
    });

    it("falls back to manifest name when StorageProvider has no label", async () => {
      const manifest = makeManifest({
        id: "storage.plugin",
        name: "Storage Name",
        ui: [
          { slot: "StorageProvider", id: "sp-1" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.storageProviders[0].contribution.label).toBe("Storage Name");
    });

    it("falls back to id when manifest name is also missing", async () => {
      const manifest = makeManifest({
        id: "storage.plugin",
        name: null as any,
        ui: [
          { slot: "StorageProvider" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.storageProviders[0].contribution.label).toBe(
        "storage.plugin",
      );
    });

    it("returns null description when not a string", async () => {
      const manifest = makeManifest({
        id: "storage.plugin",
        ui: [
          { slot: "StorageProvider", label: "SP", description: 42 },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.storageProviders[0].contribution.description).toBeNull();
    });

    it("returns empty array when no storage providers", async () => {
      const api = makeMockApi([makeManifest({ id: "no-sp" })]);
      await store.init(api);

      expect(store.storageProviders).toHaveLength(0);
    });

    it("returns multiple storage providers from different plugins", async () => {
      const m1 = makeManifest({
        id: "sp1",
        ui: [{ slot: "StorageProvider", id: "a", label: "A" }] as any,
      });
      const m2 = makeManifest({
        id: "sp2",
        ui: [{ slot: "StorageProvider", id: "b", label: "B" }] as any,
      });
      const api = makeMockApi([m1, m2]);
      await store.init(api);

      expect(store.storageProviders).toHaveLength(2);
    });
  });

  // ========================================================================
  // Block picker items
  // ========================================================================

  describe("blockPickerItems", () => {
    it("returns block picker item from legacy UI entry", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Insert Chart",
            icon: "bar-chart",
            editor_command: "insertChart",
            params: { type: "bar" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const items = store.blockPickerItems;
      expect(items).toHaveLength(1);
      expect(items[0].contribution.id).toBe("bp-1");
      expect(items[0].contribution.label).toBe("Insert Chart");
      expect(items[0].contribution.icon).toBe("bar-chart");
      expect(items[0].contribution.editor_command).toBe("insertChart");
      expect(items[0].contribution.params).toEqual({ type: "bar" });
      expect(String(items[0].pluginId)).toBe("bp.plugin");
    });

    it("skips block picker items missing id", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            label: "No ID",
            editor_command: "cmd",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems).toHaveLength(0);
    });

    it("skips block picker items missing label", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            editor_command: "cmd",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems).toHaveLength(0);
    });

    it("skips block picker items with empty editor_command", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems).toHaveLength(0);
    });

    it("skips block picker items missing editor_command", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems).toHaveLength(0);
    });

    it("defaults icon to null when not a string", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
            icon: 42,
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems[0].contribution.icon).toBeNull();
    });

    it("defaults params to empty object when not provided", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems[0].contribution.params).toEqual({});
    });

    it("parses prompt with message, param_key, and default_value", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
            prompt: {
              message: "Enter URL:",
              param_key: "url",
              default_value: "https://example.com",
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      const prompt = store.blockPickerItems[0].contribution.prompt;
      expect(prompt).toBeDefined();
      expect(prompt!.message).toBe("Enter URL:");
      expect(prompt!.param_key).toBe("url");
      expect(prompt!.default_value).toBe("https://example.com");
    });

    it("defaults prompt default_value to empty string when not a string", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
            prompt: {
              message: "Enter value:",
              param_key: "val",
            },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems[0].contribution.prompt!.default_value).toBe(
        "",
      );
    });

    it("returns undefined prompt when prompt is missing required fields", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
            prompt: { message: "hello" },
            // Missing param_key
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems[0].contribution.prompt).toBeUndefined();
    });

    it("returns undefined prompt when prompt is not an object", async () => {
      const manifest = makeManifest({
        id: "bp.plugin",
        ui: [
          {
            slot: "BlockPickerItem",
            id: "bp-1",
            label: "Item",
            editor_command: "cmd",
            prompt: "not-an-object",
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      expect(store.blockPickerItems[0].contribution.prompt).toBeUndefined();
    });
  });

  // ========================================================================
  // preloadInsertCommandIcons
  // ========================================================================

  describe("preloadInsertCommandIcons", () => {
    it("loads icons for insert_command and toolbar entries", async () => {
      const manifest = makeManifest({
        id: "icon.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext1",
            node_type: "InlineAtom",
            insert_command: { label: "Cmd", icon: "star" },
          },
          {
            slot: "EditorExtension",
            extension_id: "ext2",
            node_type: "InlineMark",
            toolbar: { icon: "bold", label: "Bold" },
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      await store.preloadInsertCommandIcons();

      expect(mocks.loadPluginIcon).toHaveBeenCalledWith("star");
      expect(mocks.loadPluginIcon).toHaveBeenCalledWith("bold");
    });

    it("skips entries without icons", async () => {
      const manifest = makeManifest({
        id: "icon.plugin",
        ui: [
          {
            slot: "EditorExtension",
            extension_id: "ext1",
            node_type: "InlineAtom",
            insert_command: { label: "Cmd" },
            // No icon
          },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      await store.preloadInsertCommandIcons();

      expect(mocks.loadPluginIcon).not.toHaveBeenCalled();
    });

    it("skips non-EditorExtension UI entries", async () => {
      const manifest = makeManifest({
        id: "icon.plugin",
        ui: [
          { slot: "SettingsTab", label: "Tab", icon: "settings" },
        ] as any,
      });
      const api = makeMockApi([manifest]);
      await store.init(api);

      await store.preloadInsertCommandIcons();

      expect(mocks.loadPluginIcon).not.toHaveBeenCalled();
    });
  });

  // ========================================================================
  // getPluginStore returns consistent reference
  // ========================================================================

  describe("getPluginStore", () => {
    it("returns an object with all expected properties", () => {
      const s = getPluginStore();
      expect(s).toHaveProperty("allManifests");
      expect(s).toHaveProperty("manifests");
      expect(s).toHaveProperty("settingsTabs");
      expect(s).toHaveProperty("leftSidebarTabs");
      expect(s).toHaveProperty("rightSidebarTabs");
      expect(s).toHaveProperty("commandPaletteItems");
      expect(s).toHaveProperty("commandPaletteOwner");
      expect(s).toHaveProperty("leftSidebarContextMenuOwner");
      expect(s).toHaveProperty("statusBarItems");
      expect(s).toHaveProperty("workspaceProviders");
      expect(s).toHaveProperty("storageProviders");
      expect(s).toHaveProperty("blockPickerItems");
      expect(s).toHaveProperty("editorInsertCommands");
      expect(s).toHaveProperty("markToolbarEntries");
      expect(s).toHaveProperty("preloadInsertCommandIcons");
      expect(s).toHaveProperty("isPluginEnabled");
      expect(s).toHaveProperty("setPluginEnabled");
      expect(s).toHaveProperty("clearPluginEnabled");
      expect(s).toHaveProperty("hydrateDisabledPlugins");
      expect(s).toHaveProperty("init");
      expect(s).toHaveProperty("setRuntimeManifestOverride");
      expect(s).toHaveProperty("clearRuntimeManifestOverride");
      expect(s).toHaveProperty("clearRuntimeManifestOverrides");
    });
  });
});
