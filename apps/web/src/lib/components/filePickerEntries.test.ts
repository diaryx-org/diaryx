import { describe, expect, it } from "vitest";

import { collectUniqueEntries, filterEntries } from "./filePickerEntries";

describe("filePickerEntries", () => {
  it("deduplicates entries by canonical path", () => {
    const tree = {
      path: "/workspace/README.md",
      name: "README",
      children: [
        {
          path: "/workspace/notes/a.md",
          name: "a",
          children: [],
        },
        {
          path: "/workspace/README.md",
          name: "README duplicate",
          children: [],
        },
      ],
    };

    expect(collectUniqueEntries(tree as any)).toEqual([
      { path: "/workspace/README.md", name: "README" },
      { path: "/workspace/notes/a.md", name: "a" },
    ]);
  });

  it("filters by search text and excluded paths", () => {
    const entries = [
      { path: "/workspace/README.md", name: "README" },
      { path: "/workspace/notes/a.md", name: "Alpha" },
      { path: "/workspace/notes/b.md", name: "Beta" },
    ];

    expect(
      filterEntries(entries, "alp", ["/workspace/README.md"]),
    ).toEqual([{ path: "/workspace/notes/a.md", name: "Alpha" }]);
  });
});
