import { describe, expect, it } from "vitest";

import {
  computeResizeWidth,
  getPendingDeleteDescription,
  getPendingDeleteName,
  normalizeFrontmatter,
} from "./App.helpers";

// ---------------------------------------------------------------------------
// getPendingDeleteName
// ---------------------------------------------------------------------------

describe("getPendingDeleteName", () => {
  it("returns filename without .md for single path", () => {
    expect(getPendingDeleteName(["workspace/notes/hello.md"])).toBe("hello");
  });

  it("returns count for multiple paths", () => {
    expect(getPendingDeleteName(["a.md", "b.md", "c.md"])).toBe(
      "3 selected entries",
    );
  });

  it("returns empty string for no paths", () => {
    expect(getPendingDeleteName([])).toBe("");
  });

  it("handles path with no directory component", () => {
    expect(getPendingDeleteName(["readme.md"])).toBe("readme");
  });

  it("handles deeply nested paths", () => {
    expect(getPendingDeleteName(["a/b/c/d/entry.md"])).toBe("entry");
  });

  it("handles filename without .md extension", () => {
    expect(getPendingDeleteName(["folder/notes.txt"])).toBe("notes.txt");
  });
});

// ---------------------------------------------------------------------------
// getPendingDeleteDescription
// ---------------------------------------------------------------------------

describe("getPendingDeleteDescription", () => {
  it("returns correct text for single entry without descendants", () => {
    const desc = getPendingDeleteDescription(["workspace/hello.md"], false);
    expect(desc).toBe(
      'Are you sure you want to delete "hello"? This action cannot be undone.',
    );
  });

  it("returns correct text for single entry with descendants", () => {
    const desc = getPendingDeleteDescription(["workspace/hello.md"], true);
    expect(desc).toBe(
      'Are you sure you want to delete "hello" and its descendants? This action cannot be undone.',
    );
  });

  it("returns correct text for multiple entries without descendants", () => {
    const desc = getPendingDeleteDescription(["a.md", "b.md"], false);
    expect(desc).toBe(
      "Are you sure you want to delete 2 selected entries? This action cannot be undone.",
    );
  });

  it("returns correct text for multiple entries with descendants", () => {
    const desc = getPendingDeleteDescription(["a.md", "b.md", "c.md"], true);
    expect(desc).toBe(
      "Are you sure you want to delete 3 selected entries and their descendants? This action cannot be undone.",
    );
  });

  it("returns empty string for no paths", () => {
    expect(getPendingDeleteDescription([], false)).toBe("");
    expect(getPendingDeleteDescription([], true)).toBe("");
  });
});

// ---------------------------------------------------------------------------
// computeResizeWidth
// ---------------------------------------------------------------------------

describe("computeResizeWidth", () => {
  it("calculates left sidebar resize correctly (drag right increases width)", () => {
    // Start at 288px, pointer moves 50px to the right
    expect(computeResizeWidth(288, 300, 350, "left")).toBe(338);
  });

  it("calculates left sidebar resize correctly (drag left decreases width)", () => {
    expect(computeResizeWidth(288, 300, 250, "left")).toBe(238);
  });

  it("calculates right sidebar resize correctly (drag left increases width)", () => {
    // For the right sidebar, moving left (currentX < startX) increases width
    expect(computeResizeWidth(288, 800, 750, "right")).toBe(338);
  });

  it("calculates right sidebar resize correctly (drag right decreases width)", () => {
    expect(computeResizeWidth(288, 800, 850, "right")).toBe(238);
  });

  it("returns startWidth when pointer has not moved", () => {
    expect(computeResizeWidth(300, 500, 500, "left")).toBe(300);
    expect(computeResizeWidth(300, 500, 500, "right")).toBe(300);
  });
});

// ---------------------------------------------------------------------------
// normalizeFrontmatter
// ---------------------------------------------------------------------------

describe("normalizeFrontmatter", () => {
  it("returns empty object for null/undefined", () => {
    expect(normalizeFrontmatter(null)).toEqual({});
    expect(normalizeFrontmatter(undefined)).toEqual({});
  });

  it("converts a Map to a plain object", () => {
    const map = new Map<string, unknown>([
      ["title", "Hello"],
      ["tags", ["a", "b"]],
    ]);
    expect(normalizeFrontmatter(map)).toEqual({
      title: "Hello",
      tags: ["a", "b"],
    });
  });

  it("returns a plain object as-is", () => {
    const obj = { title: "World", draft: true };
    expect(normalizeFrontmatter(obj)).toBe(obj);
  });

  it("returns empty object for empty string (falsy)", () => {
    expect(normalizeFrontmatter("")).toEqual({});
  });

  it("returns empty object for zero (falsy)", () => {
    expect(normalizeFrontmatter(0)).toEqual({});
  });
});
