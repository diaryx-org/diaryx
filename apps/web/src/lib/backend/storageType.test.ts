import { afterEach, describe, expect, it, vi } from "vitest";

function makeNavigatorWithDirectory(
  getDirectory: () => Promise<FileSystemDirectoryHandle>,
): Navigator {
  return {
    storage: {
      getDirectory,
    },
  } as Navigator;
}

describe("storageType", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("downgrades OPFS to IndexedDB when the runtime probe fails", async () => {
    vi.resetModules();
    vi.stubGlobal("localStorage", {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    });
    vi.stubGlobal(
      "navigator",
      makeNavigatorWithDirectory(async () => {
        throw new DOMException("Blocked", "UnknownError");
      }),
    );

    const mod = await import("./storageType");

    expect(await mod.isOpfsUsable()).toBe(false);
    expect(await mod.resolveStorageType("opfs")).toBe("indexeddb");
  });

  it("keeps OPFS when the runtime probe succeeds", async () => {
    vi.resetModules();
    vi.stubGlobal("localStorage", {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    });
    vi.stubGlobal(
      "navigator",
      makeNavigatorWithDirectory(async () => ({}) as FileSystemDirectoryHandle),
    );

    const mod = await import("./storageType");

    expect(await mod.isOpfsUsable()).toBe(true);
    expect(await mod.resolveStorageType("opfs")).toBe("opfs");
  });

  it("preserves non-OPFS storage types without probing", async () => {
    vi.resetModules();
    vi.stubGlobal("localStorage", {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    });
    const getDirectory = vi.fn(async () => ({}) as FileSystemDirectoryHandle);
    vi.stubGlobal("navigator", makeNavigatorWithDirectory(getDirectory));

    const mod = await import("./storageType");

    expect(await mod.resolveStorageType("indexeddb")).toBe("indexeddb");
    expect(getDirectory).not.toHaveBeenCalled();
  });
});
