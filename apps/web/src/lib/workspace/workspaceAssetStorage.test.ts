import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  readFile: vi.fn(),
  writeFile: vi.fn(),
  deleteFile: vi.fn(),
  getFilesystemTree: vi.fn(),
  readBinary: vi.fn(),
  writeBinary: vi.fn(),
  getBackend: vi.fn(),
  getBackendSync: vi.fn(),
  createApi: vi.fn(),
}));

const mockBackend = {
  readBinary: mocks.readBinary,
  writeBinary: mocks.writeBinary,
};

mocks.getBackend.mockResolvedValue(mockBackend);
mocks.getBackendSync.mockReturnValue(mockBackend);
mocks.createApi.mockReturnValue({
  readFile: mocks.readFile,
  writeFile: mocks.writeFile,
  deleteFile: mocks.deleteFile,
  getFilesystemTree: mocks.getFilesystemTree,
});

vi.mock("$lib/backend", () => ({
  getBackend: mocks.getBackend,
  getBackendSync: mocks.getBackendSync,
  createApi: mocks.createApi,
}));

import {
  getPluginInstallPath,
  getPluginStoragePath,
  getThemeSettingsPath,
  getThemeModePath,
  getThemeLibraryPath,
  getTypographySettingsPath,
  getTypographyLibraryPath,
  tryGetWorkspaceBackendSync,
  readWorkspaceText,
  writeWorkspaceText,
  readWorkspaceBinary,
  writeWorkspaceBinary,
  deleteWorkspacePath,
  listWorkspaceFiles,
  deleteWorkspaceTree,
} from "./workspaceAssetStorage";

const mockReadFile = mocks.readFile;
const mockWriteFile = mocks.writeFile;
const mockDeleteFile = mocks.deleteFile;
const mockGetFilesystemTree = mocks.getFilesystemTree;
const mockReadBinary = mocks.readBinary;
const mockWriteBinary = mocks.writeBinary;

import { getBackendSync } from "$lib/backend";

describe("workspaceAssetStorage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("path helpers", () => {
    it("getPluginInstallPath returns correct path", () => {
      expect(getPluginInstallPath("my-plugin")).toBe(".diaryx/plugins/my-plugin/plugin.wasm");
    });

    it("getPluginStoragePath returns base64url-encoded key path", () => {
      const path = getPluginStoragePath("my-plugin", "counter");
      expect(path).toMatch(/^\.diaryx\/plugins\/my-plugin\/state\/.+\.bin$/);
    });

    it("getPluginStoragePath encodes different keys differently", () => {
      const path1 = getPluginStoragePath("p", "key-a");
      const path2 = getPluginStoragePath("p", "key-b");
      expect(path1).not.toBe(path2);
    });

    it("getThemeSettingsPath", () => {
      expect(getThemeSettingsPath()).toBe(".diaryx/themes/settings.json");
    });

    it("getThemeModePath", () => {
      expect(getThemeModePath()).toBe(".diaryx/themes/mode.json");
    });

    it("getThemeLibraryPath", () => {
      expect(getThemeLibraryPath()).toBe(".diaryx/themes/library.json");
    });

    it("getTypographySettingsPath", () => {
      expect(getTypographySettingsPath()).toBe(".diaryx/typographies/settings.json");
    });

    it("getTypographyLibraryPath", () => {
      expect(getTypographyLibraryPath()).toBe(".diaryx/typographies/library.json");
    });
  });

  describe("tryGetWorkspaceBackendSync", () => {
    it("returns backend when available", () => {
      expect(tryGetWorkspaceBackendSync()).toBe(mockBackend);
    });

    it("returns null when getBackendSync throws", () => {
      vi.mocked(getBackendSync).mockImplementationOnce(() => {
        throw new Error("no backend");
      });
      expect(tryGetWorkspaceBackendSync()).toBeNull();
    });
  });

  describe("readWorkspaceText", () => {
    it("reads file with normalized path", async () => {
      mockReadFile.mockResolvedValueOnce("hello");
      const result = await readWorkspaceText("./some//path.txt");
      expect(mockReadFile).toHaveBeenCalledWith("some/path.txt");
      expect(result).toBe("hello");
    });

    it("returns null for NotFound errors", async () => {
      mockReadFile.mockRejectedValueOnce(new Error("NotFound"));
      expect(await readWorkspaceText("missing.txt")).toBeNull();
    });

    it("returns null for 'not found' errors", async () => {
      mockReadFile.mockRejectedValueOnce(new Error("file not found"));
      expect(await readWorkspaceText("missing.txt")).toBeNull();
    });

    it("returns null for 'could not be found' errors", async () => {
      mockReadFile.mockRejectedValueOnce(new Error("file could not be found"));
      expect(await readWorkspaceText("missing.txt")).toBeNull();
    });

    it("returns null for 'object can not be found' errors", async () => {
      mockReadFile.mockRejectedValueOnce(new Error("object can not be found"));
      expect(await readWorkspaceText("missing.txt")).toBeNull();
    });

    it("rethrows non-missing-file errors", async () => {
      mockReadFile.mockRejectedValueOnce(new Error("permission denied"));
      await expect(readWorkspaceText("secret.txt")).rejects.toThrow("permission denied");
    });
  });

  describe("writeWorkspaceText", () => {
    it("writes file with normalized path", async () => {
      await writeWorkspaceText("./foo//bar.txt", "content");
      expect(mockWriteFile).toHaveBeenCalledWith("foo/bar.txt", "content");
    });
  });

  describe("readWorkspaceBinary", () => {
    it("reads binary data with normalized path", async () => {
      const data = new Uint8Array([1, 2, 3]);
      mockReadBinary.mockResolvedValueOnce(data);
      const result = await readWorkspaceBinary("./data.bin");
      expect(mockReadBinary).toHaveBeenCalledWith("data.bin");
      expect(result).toBe(data);
    });

    it("returns null for missing file errors", async () => {
      mockReadBinary.mockRejectedValueOnce(new Error("NotFound"));
      expect(await readWorkspaceBinary("missing.bin")).toBeNull();
    });

    it("rethrows non-missing-file errors", async () => {
      mockReadBinary.mockRejectedValueOnce(new Error("disk error"));
      await expect(readWorkspaceBinary("bad.bin")).rejects.toThrow("disk error");
    });
  });

  describe("writeWorkspaceBinary", () => {
    it("writes binary data with normalized path", async () => {
      const data = new Uint8Array([4, 5, 6]);
      await writeWorkspaceBinary("./output.bin", data);
      expect(mockWriteBinary).toHaveBeenCalledWith("output.bin", data);
    });
  });

  describe("deleteWorkspacePath", () => {
    it("deletes file with normalized path", async () => {
      await deleteWorkspacePath("./file.txt");
      expect(mockDeleteFile).toHaveBeenCalledWith("file.txt");
    });

    it("silently ignores missing file errors", async () => {
      mockDeleteFile.mockRejectedValueOnce(new Error("NotFound"));
      await expect(deleteWorkspacePath("gone.txt")).resolves.toBeUndefined();
    });

    it("rethrows non-missing-file errors", async () => {
      mockDeleteFile.mockRejectedValueOnce(new Error("permission denied"));
      await expect(deleteWorkspacePath("protected.txt")).rejects.toThrow("permission denied");
    });
  });

  describe("listWorkspaceFiles", () => {
    it("returns leaf paths from the tree", async () => {
      mockGetFilesystemTree.mockResolvedValueOnce({
        name: "root", path: "root", children: [
          { name: "a.txt", path: "root/a.txt", children: [] },
          { name: "sub", path: "root/sub", children: [
            { name: "b.txt", path: "root/sub/b.txt", children: [] },
          ]},
        ],
      });

      expect(await listWorkspaceFiles("root")).toEqual(["root/a.txt", "root/sub/b.txt"]);
    });

    it("returns empty array when tree is null (missing dir)", async () => {
      mockGetFilesystemTree.mockRejectedValueOnce(new Error("NotFound"));
      expect(await listWorkspaceFiles("missing")).toEqual([]);
    });

    it("skips nodes with empty path", async () => {
      mockGetFilesystemTree.mockResolvedValueOnce({
        name: "root", path: "", children: [
          { name: "leaf", path: "", children: [] },
        ],
      });
      expect(await listWorkspaceFiles("root")).toEqual([]);
    });
  });

  describe("deleteWorkspaceTree", () => {
    it("deletes all files in the tree", async () => {
      mockGetFilesystemTree.mockResolvedValueOnce({
        name: "dir", path: "dir", children: [
          { name: "a.txt", path: "dir/a.txt", children: [] },
          { name: "b.txt", path: "dir/b.txt", children: [] },
        ],
      });

      await deleteWorkspaceTree("dir");
      expect(mockDeleteFile).toHaveBeenCalledTimes(2);
      expect(mockDeleteFile).toHaveBeenCalledWith("dir/a.txt");
      expect(mockDeleteFile).toHaveBeenCalledWith("dir/b.txt");
    });

    it("does nothing when tree is empty", async () => {
      mockGetFilesystemTree.mockRejectedValueOnce(new Error("NotFound"));
      await deleteWorkspaceTree("empty");
      expect(mockDeleteFile).not.toHaveBeenCalled();
    });
  });

  describe("path normalization", () => {
    it("normalizes backslashes", async () => {
      mockReadFile.mockResolvedValueOnce("ok");
      await readWorkspaceText("foo\\bar\\baz.txt");
      expect(mockReadFile).toHaveBeenCalledWith("foo/bar/baz.txt");
    });

    it("strips leading ./", async () => {
      mockReadFile.mockResolvedValueOnce("ok");
      await readWorkspaceText("./foo/bar.txt");
      expect(mockReadFile).toHaveBeenCalledWith("foo/bar.txt");
    });

    it("collapses multiple slashes", async () => {
      mockReadFile.mockResolvedValueOnce("ok");
      await readWorkspaceText("foo///bar.txt");
      expect(mockReadFile).toHaveBeenCalledWith("foo/bar.txt");
    });

    it("strips leading slashes", async () => {
      mockReadFile.mockResolvedValueOnce("ok");
      await readWorkspaceText("///foo/bar.txt");
      expect(mockReadFile).toHaveBeenCalledWith("foo/bar.txt");
    });
  });
});
