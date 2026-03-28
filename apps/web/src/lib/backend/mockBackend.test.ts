import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBackend } from "./mockBackend";
import { listWorkspaceFiles } from "../workspace/workspaceAssetStorage";

const backendHolder = vi.hoisted(() => ({
  backend: null as MockBackend | null,
}));

vi.mock("$lib/backend", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/backend")>();
  return {
    ...actual,
    getBackend: async () => {
      if (!backendHolder.backend) {
        throw new Error("Mock backend test backend not initialized");
      }
      return backendHolder.backend;
    },
    getBackendSync: () => {
      if (!backendHolder.backend) {
        throw new Error("Mock backend test backend not initialized");
      }
      return backendHolder.backend;
    },
    isTauri: () => false,
  };
});

function makeFileMap(): Map<string, string> {
  return new Map<string, string>([
    [
      "workspace/index.md",
      "---\ntitle: Workspace\ncontents: [welcome.md]\n---\n",
    ],
    [
      "workspace/welcome.md",
      "---\ntitle: Welcome\n---\nHello",
    ],
    [
      ".diaryx/plugins/plugin.a/plugin.wasm",
      "wasm-a",
    ],
    [
      ".diaryx/plugins/.hidden/plugin.wasm",
      "wasm-hidden",
    ],
    [
      ".diaryx/plugins/plugin.b/README.md",
      "---\ntitle: Plugin B\n---\nReadme",
    ],
  ]);
}

async function createMockBackend(): Promise<MockBackend> {
  const backend = new MockBackend();
  backend.loadFiles(makeFileMap());
  await backend.init();
  return backend;
}

describe("MockBackend", () => {
  beforeEach(() => {
    vi.resetModules();
    backendHolder.backend = null;
  });

  afterEach(() => {
    vi.resetModules();
    backendHolder.backend = null;
  });

  it("returns a Tree response for GetFilesystemTree", async () => {
    const backend = await createMockBackend();

    const response = await backend.execute({
      type: "GetFilesystemTree",
      params: { path: ".diaryx/plugins", show_hidden: true, depth: null },
    } as any);

    expect(response.type).toBe("Tree");
    if (response.type !== "Tree") {
      throw new Error("Expected Tree response");
    }
    expect(response.data.path).toBe(".diaryx/plugins");
  });

  it("honors subtree selection and depth for GetFilesystemTree", async () => {
    const backend = await createMockBackend();

    const response = await backend.execute({
      type: "GetFilesystemTree",
      params: { path: ".diaryx/plugins", show_hidden: true, depth: 1 },
    } as any);

    expect(response.type).toBe("Tree");
    if (response.type !== "Tree") {
      throw new Error("Expected Tree response");
    }

    expect(response.data.path).toBe(".diaryx/plugins");
    expect(response.data.children.map((child) => child.path)).toEqual([
      ".diaryx/plugins/.hidden",
      ".diaryx/plugins/plugin.a",
      ".diaryx/plugins/plugin.b",
    ]);
    expect(response.data.children.every((child) => child.children.length === 0)).toBe(true);
  });

  it("filters hidden filesystem entries when show_hidden is false", async () => {
    const backend = await createMockBackend();

    const response = await backend.execute({
      type: "GetFilesystemTree",
      params: { path: ".diaryx/plugins", show_hidden: false, depth: null },
    } as any);

    expect(response.type).toBe("Tree");
    if (response.type !== "Tree") {
      throw new Error("Expected Tree response");
    }

    expect(response.data.children.map((child) => child.path)).toEqual([
      ".diaryx/plugins/plugin.a",
      ".diaryx/plugins/plugin.b",
    ]);
  });

  it("throws for unsupported commands instead of returning Ok", async () => {
    const backend = await createMockBackend();

    await expect(
      backend.execute({ type: "DefinitelyUnsupportedCommand" } as any),
    ).rejects.toThrow("MockBackend does not implement command: DefinitelyUnsupportedCommand");
  });

  it("lists workspace plugin files without a Tree/Ok mismatch", async () => {
    const backend = await createMockBackend();
    backendHolder.backend = backend;

    await expect(listWorkspaceFiles(".diaryx/plugins")).resolves.toEqual([
      ".diaryx/plugins/.hidden/plugin.wasm",
      ".diaryx/plugins/plugin.a/plugin.wasm",
      ".diaryx/plugins/plugin.b/README.md",
    ]);
  });
});
