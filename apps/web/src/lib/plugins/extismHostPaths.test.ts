import { describe, expect, it } from "vitest";

import { normalizeExtismHostPath } from "./extismHostPaths";

describe("normalizeExtismHostPath", () => {
  it("strips leading current-directory prefixes", () => {
    expect(normalizeExtismHostPath("./live-propagation.md")).toBe("live-propagation.md");
    expect(normalizeExtismHostPath("./nested/entry.md")).toBe("nested/entry.md");
  });

  it("collapses redundant separators and current-directory segments", () => {
    expect(normalizeExtismHostPath("nested//./entry.md")).toBe("nested/entry.md");
    expect(normalizeExtismHostPath(".//nested///child.md")).toBe("nested/child.md");
  });

  it("preserves the workspace root sentinel", () => {
    expect(normalizeExtismHostPath(".")).toBe(".");
    expect(normalizeExtismHostPath("")).toBe(".");
    expect(normalizeExtismHostPath(undefined)).toBe(".");
  });
});
