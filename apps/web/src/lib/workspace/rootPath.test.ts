import { describe, expect, it, vi } from "vitest";

import {
  getWorkspaceDirectoryPath,
  resolveWorkspaceValidationRootPath,
} from "./rootPath";

describe("rootPath", () => {
  describe("getWorkspaceDirectoryPath", () => {
    it("returns directories unchanged", () => {
      expect(getWorkspaceDirectoryPath("/workspace")).toBe("/workspace");
    });

    it("returns parent directory for nonstandard root markdown filenames", () => {
      expect(getWorkspaceDirectoryPath("/workspace/Diaryx.md")).toBe("/workspace");
    });

    it("returns current directory for bare markdown filenames", () => {
      expect(getWorkspaceDirectoryPath("Diaryx.md")).toBe(".");
    });
  });

  describe("resolveWorkspaceValidationRootPath", () => {
    it("prefers the loaded tree path", async () => {
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn(),
      };

      await expect(
        resolveWorkspaceValidationRootPath(
          api as any,
          { path: "/workspace/Diaryx.md" } as any,
          "/workspace",
        ),
      ).resolves.toBe("/workspace/Diaryx.md");
      expect(api.resolveWorkspaceRootIndexPath).not.toHaveBeenCalled();
    });

    it("falls back to backend root-index resolution when no tree is loaded", async () => {
      const api = {
        resolveWorkspaceRootIndexPath: vi.fn().mockResolvedValue("/workspace/Diaryx.md"),
      };

      await expect(
        resolveWorkspaceValidationRootPath(api as any, null, "/workspace/Diaryx.md"),
      ).resolves.toBe("/workspace/Diaryx.md");
      expect(api.resolveWorkspaceRootIndexPath).toHaveBeenCalledWith("/workspace/Diaryx.md");
    });
  });
});
