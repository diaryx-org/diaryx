import { beforeEach, describe, expect, it, vi } from "vitest";

type LocalWorkspaceRegistryModule = typeof import("./localWorkspaceRegistry.svelte");

/**
 * Helper: reset module state and optionally seed localStorage before importing.
 * Because the module uses Svelte 5 $state at the top level, we must reset modules
 * between tests so each test gets a fresh reactive snapshot.
 */
async function loadRegistry(
  initialWorkspaces?: Array<Record<string, unknown>>,
  currentWorkspaceId?: string,
): Promise<LocalWorkspaceRegistryModule> {
  vi.resetModules();
  localStorage.clear();

  if (initialWorkspaces) {
    localStorage.setItem(
      "diaryx_local_workspaces",
      JSON.stringify(initialWorkspaces),
    );
  }
  if (currentWorkspaceId) {
    localStorage.setItem("diaryx_current_workspace", currentWorkspaceId);
  }

  return await import("./localWorkspaceRegistry.svelte");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeWorkspace(overrides: Record<string, unknown> = {}) {
  return {
    id: "local-aaa",
    name: "Test Journal",
    isLocal: true,
    downloadedAt: 1000,
    lastOpenedAt: 2000,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("localWorkspaceRegistry", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  // ========================================================================
  // createLocalWorkspace
  // ========================================================================

  describe("createLocalWorkspace", () => {
    it("creates a workspace with a local- prefixed UUID id", async () => {
      const registry = await loadRegistry();
      const ws = registry.createLocalWorkspace("My Diary");

      expect(ws.id).toMatch(/^local-/);
      expect(ws.name).toBe("My Diary");
      expect(ws.isLocal).toBe(true);
      expect(ws.downloadedAt).toBeGreaterThan(0);
      expect(ws.lastOpenedAt).toBeGreaterThan(0);
    });

    it("adds the workspace to the registry", async () => {
      const registry = await loadRegistry();
      const ws = registry.createLocalWorkspace("New Workspace");

      const all = registry.getLocalWorkspaces();
      expect(all).toHaveLength(1);
      expect(all[0].id).toBe(ws.id);
    });

    it("persists the workspace to localStorage", async () => {
      const registry = await loadRegistry();
      registry.createLocalWorkspace("Persisted");

      const stored = JSON.parse(localStorage.getItem("diaryx_local_workspaces")!);
      expect(stored).toHaveLength(1);
      expect(stored[0].name).toBe("Persisted");
    });

    it("uses the provided storageType when given", async () => {
      const registry = await loadRegistry();
      const ws = registry.createLocalWorkspace("Indexed", "indexeddb");

      expect(ws.storageType).toBe("indexeddb");
    });

    it("sets path when provided", async () => {
      const registry = await loadRegistry();
      const ws = registry.createLocalWorkspace("With Path", undefined, "/some/path");

      expect(ws.path).toBe("/some/path");
    });
  });

  // ========================================================================
  // addLocalWorkspace / removeLocalWorkspace
  // ========================================================================

  describe("addLocalWorkspace", () => {
    it("adds a new workspace to the registry", async () => {
      const registry = await loadRegistry();
      registry.addLocalWorkspace({ id: "local-123", name: "Added" });

      const ws = registry.getLocalWorkspace("local-123");
      expect(ws).not.toBeNull();
      expect(ws!.name).toBe("Added");
      expect(ws!.isLocal).toBe(true);
    });

    it("updates name if workspace already exists", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.addLocalWorkspace({ id: "local-aaa", name: "Renamed" });

      const ws = registry.getLocalWorkspace("local-aaa");
      expect(ws!.name).toBe("Renamed");
      // Should not duplicate
      expect(registry.getLocalWorkspaces()).toHaveLength(1);
    });

    it("updates storageType on existing workspace when provided", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.addLocalWorkspace({ id: "local-aaa", name: "Test", storageType: "indexeddb" });

      const ws = registry.getLocalWorkspace("local-aaa");
      expect(ws!.storageType).toBe("indexeddb");
    });

    it("updates path on existing workspace when provided", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.addLocalWorkspace({ id: "local-aaa", name: "Test", path: "/new/path" });

      const ws = registry.getLocalWorkspace("local-aaa");
      expect(ws!.path).toBe("/new/path");
    });
  });

  describe("removeLocalWorkspace", () => {
    it("removes a workspace from the registry", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getLocalWorkspaces()).toHaveLength(1);

      registry.removeLocalWorkspace("local-aaa");
      expect(registry.getLocalWorkspaces()).toHaveLength(0);
    });

    it("clears current workspace if removed workspace was current", async () => {
      const registry = await loadRegistry([makeWorkspace()], "local-aaa");

      expect(registry.getCurrentWorkspaceId()).toBe("local-aaa");
      registry.removeLocalWorkspace("local-aaa");
      expect(registry.getCurrentWorkspaceId()).toBeNull();
    });

    it("does not affect current workspace if a different workspace is removed", async () => {
      const ws1 = makeWorkspace({ id: "local-aaa" });
      const ws2 = makeWorkspace({ id: "local-bbb", name: "Other" });
      const registry = await loadRegistry([ws1, ws2], "local-aaa");

      registry.removeLocalWorkspace("local-bbb");
      expect(registry.getCurrentWorkspaceId()).toBe("local-aaa");
      expect(registry.getLocalWorkspaces()).toHaveLength(1);
    });

    it("is a no-op for non-existent workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.removeLocalWorkspace("non-existent");
      expect(registry.getLocalWorkspaces()).toHaveLength(1);
    });
  });

  // ========================================================================
  // getLocalWorkspaces / getLocalWorkspace
  // ========================================================================

  describe("getLocalWorkspaces", () => {
    it("returns empty array when no workspaces exist", async () => {
      const registry = await loadRegistry();
      expect(registry.getLocalWorkspaces()).toEqual([]);
    });

    it("returns all workspaces sorted by lastOpenedAt descending", async () => {
      const ws1 = makeWorkspace({ id: "local-1", name: "Old", lastOpenedAt: 1000 });
      const ws2 = makeWorkspace({ id: "local-2", name: "New", lastOpenedAt: 3000 });
      const ws3 = makeWorkspace({ id: "local-3", name: "Mid", lastOpenedAt: 2000 });
      const registry = await loadRegistry([ws1, ws2, ws3]);

      const all = registry.getLocalWorkspaces();
      expect(all.map((w) => w.id)).toEqual(["local-2", "local-3", "local-1"]);
    });
  });

  describe("getLocalWorkspace", () => {
    it("returns the workspace matching the given id", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      const ws = registry.getLocalWorkspace("local-aaa");
      expect(ws).not.toBeNull();
      expect(ws!.name).toBe("Test Journal");
    });

    it("returns null for non-existent id", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getLocalWorkspace("non-existent")).toBeNull();
    });
  });

  // ========================================================================
  // setCurrentWorkspaceId / getCurrentWorkspaceId
  // ========================================================================

  describe("setCurrentWorkspaceId / getCurrentWorkspaceId", () => {
    it("returns null when no current workspace is set", async () => {
      const registry = await loadRegistry();
      expect(registry.getCurrentWorkspaceId()).toBeNull();
    });

    it("persists and retrieves the current workspace id", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setCurrentWorkspaceId("local-aaa");

      expect(registry.getCurrentWorkspaceId()).toBe("local-aaa");
      expect(localStorage.getItem("diaryx_current_workspace")).toBe("local-aaa");
    });

    it("updates lastOpenedAt when setting current workspace", async () => {
      const registry = await loadRegistry([makeWorkspace({ lastOpenedAt: 100 })]);
      const before = registry.getLocalWorkspace("local-aaa")!.lastOpenedAt;

      registry.setCurrentWorkspaceId("local-aaa");
      const after = registry.getLocalWorkspace("local-aaa")!.lastOpenedAt;
      expect(after).toBeGreaterThanOrEqual(before);
    });

    it("writes workspace name to localStorage", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setCurrentWorkspaceId("local-aaa");

      expect(localStorage.getItem("diaryx-workspace-name")).toBe("Test Journal");
    });
  });

  describe("clearCurrentWorkspaceId", () => {
    it("clears the current workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()], "local-aaa");
      expect(registry.getCurrentWorkspaceId()).toBe("local-aaa");

      registry.clearCurrentWorkspaceId();
      expect(registry.getCurrentWorkspaceId()).toBeNull();
      expect(localStorage.getItem("diaryx_current_workspace")).toBeNull();
    });
  });

  // ========================================================================
  // getWorkspaceStorageType
  // ========================================================================

  describe("getWorkspaceStorageType", () => {
    it("returns the workspace-specific storage type when set", async () => {
      const registry = await loadRegistry([
        makeWorkspace({ storageType: "indexeddb" }),
      ]);
      expect(registry.getWorkspaceStorageType("local-aaa")).toBe("indexeddb");
    });

    it("falls back to global default when workspace has no storage type", async () => {
      const registry = await loadRegistry([
        makeWorkspace({ storageType: undefined }),
      ]);
      // The fallback calls getStorageType() which checks navigator/localStorage
      const result = registry.getWorkspaceStorageType("local-aaa");
      expect(typeof result).toBe("string");
    });

    it("falls back to global default for non-existent workspace", async () => {
      const registry = await loadRegistry();
      const result = registry.getWorkspaceStorageType("non-existent");
      expect(typeof result).toBe("string");
    });
  });

  // ========================================================================
  // setWorkspaceStorageType
  // ========================================================================

  describe("setWorkspaceStorageType", () => {
    it("updates the storage type for a workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setWorkspaceStorageType("local-aaa", "indexeddb");
      expect(registry.getWorkspaceStorageType("local-aaa")).toBe("indexeddb");
    });

    it("is a no-op for non-existent workspace", async () => {
      const registry = await loadRegistry();
      registry.setWorkspaceStorageType("non-existent", "indexeddb");
      // Should not throw
    });
  });

  // ========================================================================
  // setPluginMetadata / getPluginMetadata
  // ========================================================================

  describe("setPluginMetadata / getPluginMetadata", () => {
    it("returns undefined when no metadata is set", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getPluginMetadata("local-aaa", "some.plugin")).toBeUndefined();
    });

    it("sets and retrieves plugin metadata", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setPluginMetadata("local-aaa", "my.plugin", { key: "value" });

      expect(registry.getPluginMetadata("local-aaa", "my.plugin")).toEqual({ key: "value" });
    });

    it("merges metadata with existing values", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setPluginMetadata("local-aaa", "my.plugin", { a: 1 });
      registry.setPluginMetadata("local-aaa", "my.plugin", { b: 2 });

      expect(registry.getPluginMetadata("local-aaa", "my.plugin")).toEqual({ a: 1, b: 2 });
    });

    it("removes metadata when set to null", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setPluginMetadata("local-aaa", "my.plugin", { key: "value" });
      registry.setPluginMetadata("local-aaa", "my.plugin", null);

      expect(registry.getPluginMetadata("local-aaa", "my.plugin")).toBeUndefined();
    });

    it("normalizes legacy sync plugin id to diaryx.sync", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setPluginMetadata("local-aaa", "sync", {
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      });

      // Should be accessible via both old and new plugin IDs
      expect(registry.getPluginMetadata("local-aaa", "diaryx.sync")).toEqual({
        remoteWorkspaceId: "remote-1",
        serverId: "remote-1",
        syncEnabled: true,
      });
      expect(registry.getPluginMetadata("local-aaa", "sync")).toEqual({
        remoteWorkspaceId: "remote-1",
        serverId: "remote-1",
        syncEnabled: true,
      });
    });

    it("updates isLocal flag based on provider links", async () => {
      const registry = await loadRegistry([makeWorkspace({ isLocal: true })]);

      // Link to a remote workspace -> isLocal should become false
      registry.setPluginMetadata("local-aaa", "diaryx.sync", {
        remoteWorkspaceId: "remote-1",
        syncEnabled: true,
      });
      expect(registry.getLocalWorkspace("local-aaa")!.isLocal).toBe(false);

      // Remove remote link -> isLocal should become true again
      registry.setPluginMetadata("local-aaa", "diaryx.sync", null);
      expect(registry.getLocalWorkspace("local-aaa")!.isLocal).toBe(true);
    });

    it("is a no-op for non-existent workspace", async () => {
      const registry = await loadRegistry();
      // Should not throw
      registry.setPluginMetadata("non-existent", "my.plugin", { a: 1 });
    });

    it("persists normalized serverId alongside remoteWorkspaceId", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setPluginMetadata("local-aaa", "acme.cloud", {
        serverId: "remote-9",
        syncEnabled: true,
      });

      const meta = registry.getPluginMetadata("local-aaa", "acme.cloud");
      expect(meta).toEqual({
        remoteWorkspaceId: "remote-9",
        serverId: "remote-9",
        syncEnabled: true,
      });
    });
  });

  // ========================================================================
  // isWorkspaceLocal / isWorkspaceSynced
  // ========================================================================

  describe("isWorkspaceLocal", () => {
    it("returns true for a workspace in the registry", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.isWorkspaceLocal("local-aaa")).toBe(true);
    });

    it("returns false for a workspace not in the registry", async () => {
      const registry = await loadRegistry();
      expect(registry.isWorkspaceLocal("non-existent")).toBe(false);
    });
  });

  describe("isWorkspaceSynced", () => {
    it("returns false for a local-only workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.isWorkspaceSynced("local-aaa")).toBe(false);
    });

    it("returns true for a workspace with provider links", async () => {
      const registry = await loadRegistry([
        makeWorkspace({
          pluginMetadata: {
            "diaryx.sync": {
              remoteWorkspaceId: "remote-1",
              serverId: "remote-1",
              syncEnabled: true,
            },
          },
        }),
      ]);
      expect(registry.isWorkspaceSynced("local-aaa")).toBe(true);
    });
  });

  // ========================================================================
  // isWorkspaceSyncEnabled
  // ========================================================================

  describe("isWorkspaceSyncEnabled", () => {
    it("returns false when no provider link exists", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.isWorkspaceSyncEnabled("local-aaa")).toBe(false);
    });

    it("returns true when sync is enabled on the primary provider", async () => {
      const registry = await loadRegistry([
        makeWorkspace({
          pluginMetadata: {
            "diaryx.sync": {
              remoteWorkspaceId: "remote-1",
              serverId: "remote-1",
              syncEnabled: true,
            },
          },
        }),
      ]);
      expect(registry.isWorkspaceSyncEnabled("local-aaa")).toBe(true);
    });

    it("returns false when sync is explicitly disabled", async () => {
      const registry = await loadRegistry([
        makeWorkspace({
          pluginMetadata: {
            "diaryx.sync": {
              remoteWorkspaceId: "remote-1",
              serverId: "remote-1",
              syncEnabled: false,
            },
          },
        }),
      ]);
      expect(registry.isWorkspaceSyncEnabled("local-aaa")).toBe(false);
    });
  });

  // ========================================================================
  // getServerWorkspaceId
  // ========================================================================

  describe("getServerWorkspaceId", () => {
    it("returns null for a local-only workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getServerWorkspaceId("local-aaa")).toBeNull();
    });

    it("returns the remote workspace id from the primary provider", async () => {
      const registry = await loadRegistry([
        makeWorkspace({
          pluginMetadata: {
            "diaryx.sync": {
              remoteWorkspaceId: "server-uuid-1",
              serverId: "server-uuid-1",
              syncEnabled: true,
            },
          },
        }),
      ]);
      expect(registry.getServerWorkspaceId("local-aaa")).toBe("server-uuid-1");
    });
  });

  // ========================================================================
  // renameLocalWorkspace
  // ========================================================================

  describe("renameLocalWorkspace", () => {
    it("renames a workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.renameLocalWorkspace("local-aaa", "New Name");
      expect(registry.getLocalWorkspace("local-aaa")!.name).toBe("New Name");
    });

    it("updates localStorage workspace name if it is the current workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()], "local-aaa");
      registry.renameLocalWorkspace("local-aaa", "Renamed");
      expect(localStorage.getItem("diaryx-workspace-name")).toBe("Renamed");
    });

    it("is a no-op for non-existent workspace", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.renameLocalWorkspace("non-existent", "New Name");
      // Original workspace unaffected
      expect(registry.getLocalWorkspace("local-aaa")!.name).toBe("Test Journal");
    });
  });

  // ========================================================================
  // setWorkspaceIsLocal (deprecated)
  // ========================================================================

  describe("setWorkspaceIsLocal", () => {
    it("removes sync metadata when marking as local", async () => {
      const registry = await loadRegistry([
        makeWorkspace({
          isLocal: false,
          pluginMetadata: {
            "diaryx.sync": {
              remoteWorkspaceId: "remote-1",
              serverId: "remote-1",
              syncEnabled: true,
            },
          },
        }),
      ]);

      registry.setWorkspaceIsLocal("local-aaa", true);
      expect(registry.getPluginMetadata("local-aaa", "diaryx.sync")).toBeUndefined();
      expect(registry.getLocalWorkspace("local-aaa")!.isLocal).toBe(true);
    });
  });

  // ========================================================================
  // promoteLocalWorkspace
  // ========================================================================

  describe("promoteLocalWorkspace", () => {
    it("stores remote workspace id in diaryx.sync metadata", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.promoteLocalWorkspace("local-aaa", "server-uuid-42");

      const meta = registry.getPluginMetadata("local-aaa", "diaryx.sync");
      expect(meta).toEqual({
        remoteWorkspaceId: "server-uuid-42",
        serverId: "server-uuid-42",
        syncEnabled: true,
      });
      expect(registry.isWorkspaceSynced("local-aaa")).toBe(true);
    });
  });

  // ========================================================================
  // Plugin storage helpers
  // ========================================================================

  describe("getWorkspaceStoragePluginId / setWorkspaceStoragePlugin", () => {
    it("returns undefined for non-plugin storage type", async () => {
      const registry = await loadRegistry([makeWorkspace({ storageType: "opfs" })]);
      expect(registry.getWorkspaceStoragePluginId("local-aaa")).toBeUndefined();
    });

    it("sets and retrieves plugin storage id", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      registry.setWorkspaceStoragePlugin("local-aaa", "my.storage.plugin");

      expect(registry.getWorkspaceStoragePluginId("local-aaa")).toBe("my.storage.plugin");
      expect(registry.getLocalWorkspace("local-aaa")!.storageType).toBe("plugin");
    });
  });

  // ========================================================================
  // Provider links (normalizes legacy sync metadata)
  // ========================================================================

  describe("provider links", () => {
    it("normalizes legacy sync metadata into provider links", async () => {
      const registry = await loadRegistry([{
        id: "local-1",
        name: "Journal",
        isLocal: false,
        downloadedAt: 1,
        lastOpenedAt: 1,
        pluginMetadata: {
          sync: {
            serverId: "remote-1",
            syncEnabled: false,
          },
        },
      }]);

      expect(registry.getWorkspaceProviderLinks("local-1")).toEqual([{
        pluginId: "diaryx.sync",
        remoteWorkspaceId: "remote-1",
        syncEnabled: false,
      }]);
      expect(registry.getWorkspaceProviderLink("local-1", "diaryx.sync")).toEqual({
        pluginId: "diaryx.sync",
        remoteWorkspaceId: "remote-1",
        syncEnabled: false,
      });
      expect(registry.getPluginMetadata("local-1", "diaryx.sync")).toEqual({
        remoteWorkspaceId: "remote-1",
        serverId: "remote-1",
        syncEnabled: false,
      });
      expect(registry.getPrimaryWorkspaceProviderLink("local-1")).toEqual({
        pluginId: "diaryx.sync",
        remoteWorkspaceId: "remote-1",
        syncEnabled: false,
      });
    });

    it("returns empty array when no provider links exist", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getWorkspaceProviderLinks("local-aaa")).toEqual([]);
    });

    it("returns null for getWorkspaceProviderLink with unknown plugin", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getWorkspaceProviderLink("local-aaa", "unknown.plugin")).toBeNull();
    });

    it("returns null for getPrimaryWorkspaceProviderLink when none exist", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      expect(registry.getPrimaryWorkspaceProviderLink("local-aaa")).toBeNull();
    });
  });

  // ========================================================================
  // Migration: server-UUID workspaces get local IDs
  // ========================================================================

  describe("migration: server UUID to local ID", () => {
    it("migrates non-local server-UUID workspaces to local- prefixed IDs", async () => {
      const registry = await loadRegistry([{
        id: "server-uuid-123",
        name: "Migrated",
        isLocal: false,
        downloadedAt: 1,
        lastOpenedAt: 1,
      }]);

      const all = registry.getLocalWorkspaces();
      expect(all).toHaveLength(1);
      expect(all[0].id).toMatch(/^local-/);
      expect(all[0].isLocal).toBe(true);
      // Should have sync metadata with the old server UUID
      const meta = registry.getPluginMetadata(all[0].id, "diaryx.sync");
      expect(meta?.remoteWorkspaceId).toBe("server-uuid-123");
    });

    it("updates current workspace ID during migration", async () => {
      const registry = await loadRegistry(
        [{
          id: "server-uuid-456",
          name: "Current",
          isLocal: false,
          downloadedAt: 1,
          lastOpenedAt: 1,
        }],
        "server-uuid-456",
      );

      const all = registry.getLocalWorkspaces();
      const migratedId = all[0].id;
      // The current workspace ID in localStorage should have been updated during load
      expect(localStorage.getItem("diaryx_current_workspace")).toBe(migratedId);
    });
  });

  // ========================================================================
  // bootstrapDefaultWorkspace
  // ========================================================================

  describe("bootstrapDefaultWorkspace", () => {
    it("creates a default workspace when registry is empty", async () => {
      const registry = await loadRegistry();
      const ws = registry.bootstrapDefaultWorkspace();

      expect(ws.id).toMatch(/^local-/);
      expect(ws.name).toBe("My Journal");
      expect(registry.getCurrentWorkspaceId()).toBe(ws.id);
    });

    it("uses stored workspace name from localStorage", async () => {
      vi.resetModules();
      localStorage.clear();
      localStorage.setItem("diaryx-workspace-name", "Custom Name");
      const registry = await import("./localWorkspaceRegistry.svelte");

      const ws = registry.bootstrapDefaultWorkspace();
      expect(ws.name).toBe("Custom Name");
    });

    it("returns existing workspace when registry is not empty", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      const ws = registry.bootstrapDefaultWorkspace();

      expect(ws.id).toBe("local-aaa");
      // Should not create a new workspace
      expect(registry.getLocalWorkspaces()).toHaveLength(1);
    });

    it("returns current workspace when one is set", async () => {
      const ws1 = makeWorkspace({ id: "local-1", name: "First", lastOpenedAt: 1 });
      const ws2 = makeWorkspace({ id: "local-2", name: "Second", lastOpenedAt: 2 });
      const registry = await loadRegistry([ws1, ws2], "local-1");

      const ws = registry.bootstrapDefaultWorkspace();
      expect(ws.id).toBe("local-1");
    });
  });

  // ========================================================================
  // Preview mode skips localStorage writes
  // ========================================================================

  describe("preview mode", () => {
    it("skips localStorage writes when __diaryx_preview is set", async () => {
      const registry = await loadRegistry([makeWorkspace()]);
      (globalThis as any).__diaryx_preview = true;

      try {
        registry.addLocalWorkspace({ id: "local-preview", name: "Preview WS" });
        // In-memory state should be updated
        expect(registry.getLocalWorkspace("local-preview")).not.toBeNull();
        // localStorage should NOT contain the new workspace
        const stored = JSON.parse(localStorage.getItem("diaryx_local_workspaces")!);
        expect(stored.find((w: any) => w.id === "local-preview")).toBeUndefined();
      } finally {
        delete (globalThis as any).__diaryx_preview;
      }
    });
  });

  // ========================================================================
  // Edge cases
  // ========================================================================

  describe("edge cases", () => {
    it("handles corrupt localStorage gracefully", async () => {
      vi.resetModules();
      localStorage.clear();
      localStorage.setItem("diaryx_local_workspaces", "not valid json{{{");

      const registry = await import("./localWorkspaceRegistry.svelte");
      expect(registry.getLocalWorkspaces()).toEqual([]);
    });

    it("handles missing isLocal field via migration", async () => {
      const registry = await loadRegistry([{
        id: "local-old",
        name: "Old WS",
        downloadedAt: 1,
        lastOpenedAt: 1,
        // isLocal is intentionally missing
      }]);

      const ws = registry.getLocalWorkspace("local-old");
      expect(ws).not.toBeNull();
      expect(ws!.isLocal).toBe(true);
    });
  });
});
