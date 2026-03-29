import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

function makeNavigatorWithDirectory(
  getDirectory: () => Promise<FileSystemDirectoryHandle>,
): Navigator {
  return {
    storage: {
      getDirectory,
    },
  } as Navigator;
}

function makeLocalStorage(data: Record<string, string> = {}) {
  const store: Record<string, string> = { ...data };
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, val: string) => {
      store[key] = val;
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key];
    }),
    clear: vi.fn(),
    get length() {
      return Object.keys(store).length;
    },
    key: vi.fn((index: number) => Object.keys(store)[index] ?? null),
  };
}

/**
 * Helper: dynamically import the module after globals have been stubbed.
 * Each call uses `vi.resetModules()` so the module-level cached promise
 * (`opfsRuntimeSupportPromise`) starts fresh.
 */
async function freshImport() {
  vi.resetModules();
  return import("./storageType");
}

// ==========================================================================
// In-memory IndexedDB mock for file system handle persistence tests.
// jsdom does not ship a working IndexedDB, so we provide a minimal mock
// that supports open / createObjectStore / put / get / delete / transaction.
// ==========================================================================

function createMockIndexedDB() {
  const databases = new Map<string, Map<string, any>>();

  function getOrCreateStore(dbName: string, storeName: string) {
    if (!databases.has(dbName)) databases.set(dbName, new Map());
    const db = databases.get(dbName)!;
    if (!db.has(storeName)) db.set(storeName, new Map());
    return db.get(storeName)!;
  }

  const mockIDB = {
    open(dbName: string, _version?: number) {
      const request: any = {
        result: null as any,
        error: null,
        onerror: null as any,
        onsuccess: null as any,
        onupgradeneeded: null as any,
      };

      // Simulate async open
      setTimeout(() => {
        const objectStoreNames = {
          _names: new Set<string>(),
          contains(name: string) {
            return this._names.has(name);
          },
        };

        const db: any = {
          objectStoreNames,
          createObjectStore(name: string) {
            objectStoreNames._names.add(name);
            getOrCreateStore(dbName, name);
          },
          transaction(storeName: string, _mode?: string) {
            const store = getOrCreateStore(dbName, storeName);
            const tx: any = {
              oncomplete: null as any,
              onerror: null as any,
              objectStore(_name: string) {
                return {
                  put(value: any, key: string) {
                    store.set(key, structuredClone(value));
                    const putReq: any = { result: undefined, error: null, onsuccess: null, onerror: null };
                    setTimeout(() => putReq.onsuccess?.(), 0);
                    return putReq;
                  },
                  get(key: string) {
                    const getReq: any = { result: store.get(key) ?? undefined, error: null, onsuccess: null, onerror: null };
                    setTimeout(() => getReq.onsuccess?.(), 0);
                    return getReq;
                  },
                  delete(key: string) {
                    store.delete(key);
                    const delReq: any = { result: undefined, error: null, onsuccess: null, onerror: null };
                    setTimeout(() => delReq.onsuccess?.(), 0);
                    return delReq;
                  },
                };
              },
            };
            // Fire oncomplete after microtasks
            setTimeout(() => tx.oncomplete?.(), 0);
            return tx;
          },
          close() {},
        };

        request.result = db;

        // Check if store already exists
        if (databases.has(dbName) && databases.get(dbName)!.has("handles")) {
          objectStoreNames._names.add("handles");
        }

        // Fire onupgradeneeded if store doesn't exist yet
        if (!objectStoreNames.contains("handles")) {
          request.onupgradeneeded?.({ target: request });
        }

        request.onsuccess?.();
      }, 0);

      return request;
    },
  };

  return mockIDB;
}

/**
 * Create an indexedDB mock that always fires onerror on open.
 */
function createFailingIndexedDB() {
  return {
    open(_dbName: string, _version?: number) {
      const request: any = {
        result: null,
        error: new DOMException("fail"),
        onerror: null as any,
        onsuccess: null as any,
        onupgradeneeded: null as any,
      };

      setTimeout(() => {
        request.onerror?.();
      }, 0);

      return request;
    },
  };
}

describe("storageType", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  // ==========================================================================
  // isStorageTypeSupported
  // ==========================================================================
  describe("isStorageTypeSupported", () => {
    it("returns true for 'opfs' when navigator.storage.getDirectory exists", async () => {
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("opfs")).toBe(true);
    });

    it("returns false for 'opfs' when navigator is undefined", async () => {
      vi.stubGlobal("navigator", undefined);
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("opfs")).toBe(false);
    });

    it("returns false for 'opfs' when navigator.storage is missing", async () => {
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("opfs")).toBe(false);
    });

    it("returns false for 'opfs' when getDirectory is missing from storage", async () => {
      vi.stubGlobal("navigator", { storage: {} });
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("opfs")).toBe(false);
    });

    it("returns true for 'indexeddb' when indexedDB exists", async () => {
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("indexeddb")).toBe(true);
    });

    it("returns false for 'indexeddb' when indexedDB is undefined", async () => {
      vi.stubGlobal("indexedDB", undefined);
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("indexeddb")).toBe(false);
    });

    it("returns true for 'filesystem-access' when showDirectoryPicker exists", async () => {
      vi.stubGlobal("window", { showDirectoryPicker: vi.fn() });
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("filesystem-access")).toBe(true);
    });

    it("returns false for 'filesystem-access' when window is undefined", async () => {
      vi.stubGlobal("window", undefined);
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("filesystem-access")).toBe(false);
    });

    it("returns false for 'filesystem-access' when showDirectoryPicker is missing", async () => {
      vi.stubGlobal("window", {});
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("filesystem-access")).toBe(false);
    });

    it("returns true for 'memory'", async () => {
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("memory")).toBe(true);
    });

    it("returns true for 'plugin'", async () => {
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("plugin")).toBe(true);
    });

    it("returns false for an unknown type", async () => {
      const mod = await freshImport();
      expect(mod.isStorageTypeSupported("unknown-type" as any)).toBe(false);
    });
  });

  // ==========================================================================
  // isOpfsUsable
  // ==========================================================================
  describe("isOpfsUsable", () => {
    it("returns false when OPFS is not supported at all", async () => {
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(await mod.isOpfsUsable()).toBe(false);
    });

    it("returns false when getDirectory throws", async () => {
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => {
          throw new DOMException("Blocked", "UnknownError");
        }),
      );
      const mod = await freshImport();
      expect(await mod.isOpfsUsable()).toBe(false);
    });

    it("returns true when getDirectory succeeds", async () => {
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();
      expect(await mod.isOpfsUsable()).toBe(true);
    });

    it("caches the probe result (calls getDirectory only once)", async () => {
      const getDirectory = vi.fn(async () => ({}) as FileSystemDirectoryHandle);
      vi.stubGlobal("navigator", makeNavigatorWithDirectory(getDirectory));
      const mod = await freshImport();

      await mod.isOpfsUsable();
      await mod.isOpfsUsable();
      await mod.isOpfsUsable();

      expect(getDirectory).toHaveBeenCalledTimes(1);
    });

    it("resetStorageTypeRuntimeProbeForTests clears the cached probe", async () => {
      const getDirectory = vi.fn(async () => ({}) as FileSystemDirectoryHandle);
      vi.stubGlobal("navigator", makeNavigatorWithDirectory(getDirectory));
      const mod = await freshImport();

      await mod.isOpfsUsable();
      expect(getDirectory).toHaveBeenCalledTimes(1);

      mod.resetStorageTypeRuntimeProbeForTests();

      await mod.isOpfsUsable();
      expect(getDirectory).toHaveBeenCalledTimes(2);
    });
  });

  // ==========================================================================
  // getSupportedStorageTypes
  // ==========================================================================
  describe("getSupportedStorageTypes", () => {
    it("returns only indexeddb when only indexedDB is available", async () => {
      vi.stubGlobal("navigator", {}); // no OPFS
      vi.stubGlobal("window", {}); // no showDirectoryPicker
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      expect(mod.getSupportedStorageTypes()).toEqual(["indexeddb"]);
    });

    it("includes opfs when navigator.storage.getDirectory exists", async () => {
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      vi.stubGlobal("window", {});
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      const types = mod.getSupportedStorageTypes();
      expect(types).toContain("opfs");
      expect(types).toContain("indexeddb");
    });

    it("includes filesystem-access when showDirectoryPicker exists", async () => {
      vi.stubGlobal("navigator", {});
      vi.stubGlobal("window", { showDirectoryPicker: vi.fn() });
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      const types = mod.getSupportedStorageTypes();
      expect(types).toContain("filesystem-access");
    });

    it("does not include memory or plugin (they are special-purpose)", async () => {
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      vi.stubGlobal("window", { showDirectoryPicker: vi.fn() });
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      const types = mod.getSupportedStorageTypes();
      expect(types).not.toContain("memory");
      expect(types).not.toContain("plugin");
    });

    it("returns empty when nothing is supported", async () => {
      vi.stubGlobal("navigator", {}); // no OPFS
      vi.stubGlobal("window", {}); // no showDirectoryPicker
      vi.stubGlobal("indexedDB", undefined); // no indexedDB
      const mod = await freshImport();
      expect(mod.getSupportedStorageTypes()).toEqual([]);
    });
  });

  // ==========================================================================
  // getStorageType
  // ==========================================================================
  describe("getStorageType", () => {
    it("returns 'indexeddb' when localStorage is undefined", async () => {
      vi.stubGlobal("localStorage", undefined);
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("indexeddb");
    });

    it("returns stored value from localStorage when it is supported", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage({ "diaryx-storage-type": "indexeddb" }));
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("indexeddb");
    });

    it("ignores stored value if the storage type is not supported", async () => {
      // Store 'filesystem-access' but don't provide showDirectoryPicker
      vi.stubGlobal(
        "localStorage",
        makeLocalStorage({ "diaryx-storage-type": "filesystem-access" }),
      );
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      vi.stubGlobal("window", {}); // no showDirectoryPicker
      const mod = await freshImport();
      // Should fall through to OPFS since it is supported
      expect(mod.getStorageType()).toBe("opfs");
    });

    it("defaults to 'opfs' when localStorage has no stored type and OPFS is supported", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("opfs");
    });

    it("defaults to 'indexeddb' when OPFS is not supported and no stored type", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {}); // no OPFS
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("indexeddb");
    });

    it("returns stored 'memory' type from localStorage", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage({ "diaryx-storage-type": "memory" }));
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("memory");
    });

    it("returns stored 'plugin' type from localStorage", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage({ "diaryx-storage-type": "plugin" }));
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(mod.getStorageType()).toBe("plugin");
    });
  });

  // ==========================================================================
  // resolveStorageType
  // ==========================================================================
  describe("resolveStorageType", () => {
    it("downgrades OPFS to IndexedDB when the runtime probe fails", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => {
          throw new DOMException("Blocked", "UnknownError");
        }),
      );
      const mod = await freshImport();

      expect(await mod.isOpfsUsable()).toBe(false);
      expect(await mod.resolveStorageType("opfs")).toBe("indexeddb");
    });

    it("keeps OPFS when the runtime probe succeeds", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();

      expect(await mod.isOpfsUsable()).toBe(true);
      expect(await mod.resolveStorageType("opfs")).toBe("opfs");
    });

    it("preserves non-OPFS storage types without probing", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      const getDirectory = vi.fn(async () => ({}) as FileSystemDirectoryHandle);
      vi.stubGlobal("navigator", makeNavigatorWithDirectory(getDirectory));
      const mod = await freshImport();

      expect(await mod.resolveStorageType("indexeddb")).toBe("indexeddb");
      expect(getDirectory).not.toHaveBeenCalled();
    });

    it("passes through 'memory' without probing", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(await mod.resolveStorageType("memory")).toBe("memory");
    });

    it("passes through 'plugin' without probing", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(await mod.resolveStorageType("plugin")).toBe("plugin");
    });

    it("passes through 'filesystem-access' without probing", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();
      expect(await mod.resolveStorageType("filesystem-access")).toBe("filesystem-access");
    });

    it("uses getStorageType() when no preferred type is given", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage({ "diaryx-storage-type": "indexeddb" }));
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();

      // getStorageType() should return 'indexeddb' from localStorage
      expect(await mod.resolveStorageType()).toBe("indexeddb");
    });

    it("falls through to OPFS probe when no preferred type and default is OPFS", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();

      // getStorageType() returns 'opfs', then resolveStorageType probes it
      expect(await mod.resolveStorageType()).toBe("opfs");
    });
  });

  // ==========================================================================
  // setStorageType
  // ==========================================================================
  describe("setStorageType", () => {
    it("stores the type in localStorage", async () => {
      const ls = makeLocalStorage();
      vi.stubGlobal("localStorage", ls);
      vi.stubGlobal(
        "navigator",
        makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
      );
      const mod = await freshImport();

      mod.setStorageType("opfs");
      expect(ls.setItem).toHaveBeenCalledWith("diaryx-storage-type", "opfs");
    });

    it("throws when the storage type is not supported", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {}); // no OPFS
      vi.stubGlobal("window", {}); // no filesystem-access
      const mod = await freshImport();

      expect(() => mod.setStorageType("filesystem-access")).toThrow(
        'Storage type "filesystem-access" is not supported in this browser',
      );
    });

    it("throws for unsupported opfs when navigator has no storage", async () => {
      vi.stubGlobal("localStorage", makeLocalStorage());
      vi.stubGlobal("navigator", {}); // no OPFS
      const mod = await freshImport();

      expect(() => mod.setStorageType("opfs")).toThrow(
        'Storage type "opfs" is not supported in this browser',
      );
    });

    it("allows setting 'indexeddb'", async () => {
      const ls = makeLocalStorage();
      vi.stubGlobal("localStorage", ls);
      vi.stubGlobal("navigator", {});
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      const mod = await freshImport();

      mod.setStorageType("indexeddb");
      expect(ls.setItem).toHaveBeenCalledWith("diaryx-storage-type", "indexeddb");
    });

    it("allows setting 'memory'", async () => {
      const ls = makeLocalStorage();
      vi.stubGlobal("localStorage", ls);
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();

      mod.setStorageType("memory");
      expect(ls.setItem).toHaveBeenCalledWith("diaryx-storage-type", "memory");
    });

    it("allows setting 'plugin'", async () => {
      const ls = makeLocalStorage();
      vi.stubGlobal("localStorage", ls);
      vi.stubGlobal("navigator", {});
      const mod = await freshImport();

      mod.setStorageType("plugin");
      expect(ls.setItem).toHaveBeenCalledWith("diaryx-storage-type", "plugin");
    });
  });

  // ==========================================================================
  // getStorageTypeName
  // ==========================================================================
  describe("getStorageTypeName", () => {
    it("returns correct name for opfs", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeName("opfs")).toBe("Private Storage (OPFS)");
    });

    it("returns correct name for indexeddb", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeName("indexeddb")).toBe("Browser Storage (IndexedDB)");
    });

    it("returns correct name for filesystem-access", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeName("filesystem-access")).toBe("Local Folder");
    });

    it("returns correct name for plugin", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeName("plugin")).toBe("Plugin Storage");
    });

    it("returns the raw type string for unknown types", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeName("some-future-type" as any)).toBe("some-future-type");
    });

    it("returns the raw type string for memory", async () => {
      const mod = await freshImport();
      // 'memory' falls through to the default case
      expect(mod.getStorageTypeName("memory")).toBe("memory");
    });
  });

  // ==========================================================================
  // getStorageTypeDescription
  // ==========================================================================
  describe("getStorageTypeDescription", () => {
    it("returns correct description for opfs", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeDescription("opfs")).toBe(
        "High-performance storage managed by the browser. Best for most users.",
      );
    });

    it("returns correct description for indexeddb", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeDescription("indexeddb")).toBe(
        "Traditional browser database. Compatible with all browsers.",
      );
    });

    it("returns correct description for filesystem-access", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeDescription("filesystem-access")).toBe(
        "Store files in a folder on your computer. Requires Chrome or Edge.",
      );
    });

    it("returns correct description for plugin", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeDescription("plugin")).toBe(
        "Cloud storage provided by a plugin.",
      );
    });

    it("returns empty string for unknown types", async () => {
      const mod = await freshImport();
      expect(mod.getStorageTypeDescription("unknown" as any)).toBe("");
    });

    it("returns empty string for memory", async () => {
      const mod = await freshImport();
      // 'memory' falls through to the default case
      expect(mod.getStorageTypeDescription("memory")).toBe("");
    });
  });

  // ==========================================================================
  // IndexedDB file system handle persistence (global)
  // ==========================================================================
  describe("storeFileSystemHandle / getStoredFileSystemHandle / clearFileSystemHandle", () => {
    let mod: Awaited<ReturnType<typeof freshImport>>;

    beforeEach(async () => {
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      mod = await freshImport();
    });

    it("stores and retrieves a handle", async () => {
      const fakeHandle = { name: "test-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeFileSystemHandle(fakeHandle);
      const retrieved = await mod.getStoredFileSystemHandle();
      expect(retrieved).toEqual(fakeHandle);
    });

    it("returns null after clearing", async () => {
      const fakeHandle = { name: "test-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeFileSystemHandle(fakeHandle);
      await mod.clearFileSystemHandle();
      const afterClear = await mod.getStoredFileSystemHandle();
      expect(afterClear).toBeNull();
    });

    it("clears the stored handle", async () => {
      const fakeHandle = { name: "test-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeFileSystemHandle(fakeHandle);
      await mod.clearFileSystemHandle();
      const retrieved = await mod.getStoredFileSystemHandle();
      expect(retrieved).toBeNull();
    });

    it("overwrites a previously stored handle", async () => {
      const handle1 = { name: "dir1" } as unknown as FileSystemDirectoryHandle;
      const handle2 = { name: "dir2" } as unknown as FileSystemDirectoryHandle;
      await mod.storeFileSystemHandle(handle1);
      await mod.storeFileSystemHandle(handle2);
      const retrieved = await mod.getStoredFileSystemHandle();
      expect(retrieved).toEqual(handle2);
    });
  });

  // ==========================================================================
  // IndexedDB file system handle persistence (per-workspace)
  // ==========================================================================
  describe("storeWorkspaceFileSystemHandle / getWorkspaceFileSystemHandle / clearWorkspaceFileSystemHandle", () => {
    let mod: Awaited<ReturnType<typeof freshImport>>;

    beforeEach(async () => {
      vi.stubGlobal("indexedDB", createMockIndexedDB());
      mod = await freshImport();
    });

    it("stores and retrieves a workspace handle", async () => {
      const fakeHandle = { name: "ws-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeWorkspaceFileSystemHandle("ws-1", fakeHandle);
      const retrieved = await mod.getWorkspaceFileSystemHandle("ws-1");
      expect(retrieved).toEqual(fakeHandle);
    });

    it("returns null for a workspace with no stored handle", async () => {
      const retrieved = await mod.getWorkspaceFileSystemHandle("nonexistent");
      expect(retrieved).toBeNull();
    });

    it("isolates handles between workspaces", async () => {
      const handle1 = { name: "ws1-dir" } as unknown as FileSystemDirectoryHandle;
      const handle2 = { name: "ws2-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeWorkspaceFileSystemHandle("ws-1", handle1);
      await mod.storeWorkspaceFileSystemHandle("ws-2", handle2);

      expect(await mod.getWorkspaceFileSystemHandle("ws-1")).toEqual(handle1);
      expect(await mod.getWorkspaceFileSystemHandle("ws-2")).toEqual(handle2);
    });

    it("clears a workspace handle without affecting others", async () => {
      const handle1 = { name: "ws1-dir" } as unknown as FileSystemDirectoryHandle;
      const handle2 = { name: "ws2-dir" } as unknown as FileSystemDirectoryHandle;
      await mod.storeWorkspaceFileSystemHandle("ws-1", handle1);
      await mod.storeWorkspaceFileSystemHandle("ws-2", handle2);

      await mod.clearWorkspaceFileSystemHandle("ws-1");

      expect(await mod.getWorkspaceFileSystemHandle("ws-1")).toBeNull();
      expect(await mod.getWorkspaceFileSystemHandle("ws-2")).toEqual(handle2);
    });

    it("does not affect the global handle", async () => {
      const globalHandle = { name: "global" } as unknown as FileSystemDirectoryHandle;
      const wsHandle = { name: "ws" } as unknown as FileSystemDirectoryHandle;

      await mod.storeFileSystemHandle(globalHandle);
      await mod.storeWorkspaceFileSystemHandle("ws-1", wsHandle);

      expect(await mod.getStoredFileSystemHandle()).toEqual(globalHandle);
      expect(await mod.getWorkspaceFileSystemHandle("ws-1")).toEqual(wsHandle);

      await mod.clearWorkspaceFileSystemHandle("ws-1");
      expect(await mod.getStoredFileSystemHandle()).toEqual(globalHandle);
    });
  });

  // ==========================================================================
  // IndexedDB error paths
  // ==========================================================================
  describe("IndexedDB error paths", () => {
    it("storeFileSystemHandle rejects when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      await expect(
        mod.storeFileSystemHandle({} as FileSystemDirectoryHandle),
      ).rejects.toThrow("Failed to open handle database");
    });

    it("getStoredFileSystemHandle resolves null when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      const result = await mod.getStoredFileSystemHandle();
      expect(result).toBeNull();
    });

    it("clearFileSystemHandle resolves when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      await expect(mod.clearFileSystemHandle()).resolves.toBeUndefined();
    });

    it("storeWorkspaceFileSystemHandle rejects when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      await expect(
        mod.storeWorkspaceFileSystemHandle("ws-1", {} as FileSystemDirectoryHandle),
      ).rejects.toThrow("Failed to open handle database");
    });

    it("getWorkspaceFileSystemHandle resolves null when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      const result = await mod.getWorkspaceFileSystemHandle("ws-1");
      expect(result).toBeNull();
    });

    it("clearWorkspaceFileSystemHandle resolves when indexedDB.open fails", async () => {
      vi.stubGlobal("indexedDB", createFailingIndexedDB());
      const mod = await freshImport();

      await expect(mod.clearWorkspaceFileSystemHandle("ws-1")).resolves.toBeUndefined();
    });
  });
});
