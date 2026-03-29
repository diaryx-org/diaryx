import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearStarterWorkspaceRegistryCache,
  fetchStarterWorkspaceRegistry,
  getTrustedStarterWorkspaceRegistryUrls,
  normalizeStarterWorkspaceRegistryEntry,
} from "./starterWorkspaceRegistry";

const VALID_STARTER_REGISTRY_MD = `---
schema_version: 1
generated_at: "2026-03-04T00:00:00Z"
starters:
  - id: "starter.basic"
    name: "Basic"
    version: "1.0.0"
    summary: "A basic starter workspace"
    description: "Gets you started."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: null
    categories: ["general"]
    tags: ["starter"]
    icon: null
    screenshots: []
    artifact:
      url: "https://app.diaryx.org/cdn/starters/basic.zip"
      sha256: "abc"
      size: 1024
      published_at: "2026-03-04T00:00:00Z"
    file_count: 5
    bundle_id: "bundle.writer"
    includes_templates: true
---
# Starter workspace registry
`;

afterEach(() => {
  clearStarterWorkspaceRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("starterWorkspaceRegistry", () => {
  it("returns trusted URLs", () => {
    const urls = getTrustedStarterWorkspaceRegistryUrls();
    expect(urls.length).toBeGreaterThan(0);
    expect(urls[0]).toContain("/starter-workspaces/registry.md");
  });

  it("rejects untrusted URLs", async () => {
    await expect(
      fetchStarterWorkspaceRegistry("https://evil.com/registry.md"),
    ).rejects.toThrow("Untrusted starter workspace registry URL");
  });

  it("fails on non-ok HTTP response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 503 }),
    );
    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    await expect(fetchStarterWorkspaceRegistry(trusted)).rejects.toThrow(
      "Starter workspace registry fetch failed: 503",
    );
  });

  it("fails on unsupported schema version", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => '---\nschema_version: 99\ngenerated_at: "now"\nstarters: []\n---\n',
      }),
    );
    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    await expect(fetchStarterWorkspaceRegistry(trusted)).rejects.toThrow(
      "Unsupported starter workspace registry schema version",
    );
  });

  it("fails on missing frontmatter", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "no frontmatter",
      }),
    );
    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    await expect(fetchStarterWorkspaceRegistry(trusted)).rejects.toThrow("missing YAML frontmatter");
  });

  it("fails when starters is not an array", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => '---\nschema_version: 1\ngenerated_at: "now"\nstarters: "nope"\n---\n',
      }),
    );
    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    await expect(fetchStarterWorkspaceRegistry(trusted)).rejects.toThrow("'starters' must be an array");
  });

  it("parses and caches registry", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_STARTER_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    const first = await fetchStarterWorkspaceRegistry(trusted);
    const second = await fetchStarterWorkspaceRegistry(trusted);

    expect(first.schema_version).toBe(1);
    expect(first.starters).toHaveLength(1);
    expect(first.starters[0]?.id).toBe("starter.basic");
    expect(first.starters[0]?.file_count).toBe(5);
    expect(first.starters[0]?.bundle_id).toBe("bundle.writer");
    expect(first.starters[0]?.includes_templates).toBe(true);
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("clears cache so next fetch re-fetches", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_STARTER_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedStarterWorkspaceRegistryUrls()[0];
    await fetchStarterWorkspaceRegistry(trusted);
    clearStarterWorkspaceRegistryCache();
    await fetchStarterWorkspaceRegistry(trusted);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});

describe("normalizeStarterWorkspaceRegistryEntry", () => {
  it("rejects non-object input", () => {
    expect(() => normalizeStarterWorkspaceRegistryEntry("string")).toThrow(
      "starter entry must be an object",
    );
  });

  it("rejects invalid file_count", () => {
    expect(() =>
      normalizeStarterWorkspaceRegistryEntry({
        id: "x", name: "x", version: "1", summary: "x", description: "x",
        author: "x", license: "x", file_count: -1, includes_templates: true,
      }),
    ).toThrow("'file_count' must be a non-negative number");
  });

  it("rejects non-boolean includes_templates", () => {
    expect(() =>
      normalizeStarterWorkspaceRegistryEntry({
        id: "x", name: "x", version: "1", summary: "x", description: "x",
        author: "x", license: "x", file_count: 3, includes_templates: "yes",
      }),
    ).toThrow("'includes_templates' must be a boolean");
  });

  it("handles null artifact", () => {
    const entry = normalizeStarterWorkspaceRegistryEntry({
      id: "x", name: "x", version: "1", summary: "x", description: "x",
      author: "x", license: "x", file_count: 0, includes_templates: false,
      artifact: null,
    });
    expect(entry.artifact).toBeNull();
  });

  it("validates artifact fields", () => {
    expect(() =>
      normalizeStarterWorkspaceRegistryEntry({
        id: "x", name: "x", version: "1", summary: "x", description: "x",
        author: "x", license: "x", file_count: 0, includes_templates: false,
        artifact: { url: "u", sha256: "s", size: -1, published_at: "p" },
      }),
    ).toThrow("artifact.size must be a positive number");
  });

  it("rejects non-object artifact", () => {
    expect(() =>
      normalizeStarterWorkspaceRegistryEntry({
        id: "x", name: "x", version: "1", summary: "x", description: "x",
        author: "x", license: "x", file_count: 0, includes_templates: false,
        artifact: "bad",
      }),
    ).toThrow("artifact must be an object or null");
  });
});
