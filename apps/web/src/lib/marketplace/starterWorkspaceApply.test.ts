import { afterEach, describe, expect, it, vi } from "vitest";

import type { StarterWorkspaceRegistryEntry } from "./types";
import { fetchStarterWorkspaceZip } from "./starterWorkspaceApply";

function makeEntry(hasArtifact: boolean): StarterWorkspaceRegistryEntry {
  return {
    kind: "starter-workspace",
    id: "starter.basic",
    name: "Basic",
    version: "1.0.0",
    summary: "A basic starter",
    description: "Description",
    author: "Diaryx",
    license: "MIT",
    repository: null,
    categories: [],
    tags: [],
    icon: null,
    screenshots: [],
    file_count: 3,
    bundle_id: null,
    includes_templates: false,
    artifact: hasArtifact
      ? {
          url: "https://app.diaryx.org/cdn/starters/basic.zip",
          sha256: "abc",
          size: 1024,
          published_at: "2026-03-04T00:00:00Z",
        }
      : null,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("fetchStarterWorkspaceZip", () => {
  it("throws when entry has no artifact", async () => {
    await expect(fetchStarterWorkspaceZip(makeEntry(false))).rejects.toThrow(
      "Starter workspace 'starter.basic' has no artifact",
    );
  });

  it("throws on non-ok fetch response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 404 }),
    );

    await expect(fetchStarterWorkspaceZip(makeEntry(true))).rejects.toThrow(
      "Failed to fetch starter workspace artifact: 404",
    );
  });

  it("returns blob on success", async () => {
    const fakeBlob = new Blob(["zip data"]);
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        blob: async () => fakeBlob,
      }),
    );

    const result = await fetchStarterWorkspaceZip(makeEntry(true));
    expect(result).toBe(fakeBlob);
  });
});
