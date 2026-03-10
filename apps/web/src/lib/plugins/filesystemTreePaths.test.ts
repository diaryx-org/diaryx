import { describe, expect, it } from "vitest";

import { collectFilesystemTreePaths } from "./filesystemTreePaths";

describe("collectFilesystemTreePaths", () => {
  it("includes the root index file for indexed workspaces", () => {
    expect(
      collectFilesystemTreePaths({
        path: "index.md",
        children: [],
      }),
    ).toEqual(["index.md"]);
  });

  it("includes index-backed directories alongside nested files", () => {
    expect(
      collectFilesystemTreePaths({
        path: "index.md",
        children: [
          {
            path: "Projects/index.md",
            children: [
              {
                path: "Projects/note.md",
                children: [],
              },
            ],
          },
          {
            path: "attachment.png",
            children: [],
          },
        ],
      }),
    ).toEqual(["index.md", "Projects/index.md", "Projects/note.md", "attachment.png"]);
  });

  it("skips plain directories that do not have an index file", () => {
    expect(
      collectFilesystemTreePaths({
        path: ".",
        children: [
          {
            path: "notes",
            children: [
              {
                path: "notes/entry.md",
                children: [],
              },
            ],
          },
        ],
      }),
    ).toEqual(["notes/entry.md"]);
  });
});
