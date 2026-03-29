import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearTypographyRegistryCache,
  fetchTypographyRegistry,
  getTrustedTypographyRegistryUrls,
} from "./typographyRegistry";

const VALID_TYPOGRAPHY_REGISTRY_MD = `---
schema_version: 1
generated_at: "2026-03-04T00:00:00Z"
typographies:
  - id: "typography.editorial-serif"
    name: "Editorial Serif"
    version: "1.0.0"
    summary: "Book-like writing rhythm"
    description: "Comfortable serif typography for long-form writing."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["writing"]
    tags: ["serif", "comfortable"]
    styles: ["editorial"]
    icon: null
    screenshots: []
    artifact:
      url: "https://app.diaryx.org/cdn/typographies/artifacts/typography.editorial-serif/1.0.0/typography.json"
      sha256: "abc"
      size: 128
      published_at: "2026-03-04T00:00:00Z"
    typography:
      id: "typography.editorial-serif"
      name: "Editorial Serif"
      version: 1
      settings:
        fontFamily: "serif"
        baseFontSize: 18
        lineHeight: 1.8
        contentWidth: "narrow"
---
# Typography registry
`;

afterEach(() => {
  clearTypographyRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("typographyRegistry", () => {
  it("returns trusted URLs", () => {
    const urls = getTrustedTypographyRegistryUrls();
    expect(urls.length).toBeGreaterThan(0);
    expect(urls[0]).toContain("/typographies/registry.md");
  });

  it("fails on non-ok HTTP response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 500 }),
    );
    const trusted = getTrustedTypographyRegistryUrls()[0];
    await expect(fetchTypographyRegistry(trusted)).rejects.toThrow("Typography registry fetch failed: 500");
  });

  it("fails on missing frontmatter", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "no frontmatter here",
      }),
    );
    const trusted = getTrustedTypographyRegistryUrls()[0];
    await expect(fetchTypographyRegistry(trusted)).rejects.toThrow("missing YAML frontmatter");
  });

  it("fails when typographies is not an array", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "---\nschema_version: 1\ngenerated_at: \"now\"\ntypographies: \"nope\"\n---\n",
      }),
    );
    const trusted = getTrustedTypographyRegistryUrls()[0];
    await expect(fetchTypographyRegistry(trusted)).rejects.toThrow("'typographies' must be an array");
  });

  it("clears cache so next fetch re-fetches", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_TYPOGRAPHY_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedTypographyRegistryUrls()[0];
    await fetchTypographyRegistry(trusted);
    clearTypographyRegistryCache();
    await fetchTypographyRegistry(trusted);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("rejects untrusted URLs", async () => {
    await expect(
      fetchTypographyRegistry("https://example.com/typographies/registry.md"),
    ).rejects.toThrow("Untrusted typography registry URL");
  });

  it("fails on unsupported schema", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () =>
          "---\nschema_version: 7\ngenerated_at: \"\"\ntypographies: []\n---\n",
      }),
    );

    const trusted = getTrustedTypographyRegistryUrls()[0];
    await expect(fetchTypographyRegistry(trusted)).rejects.toThrow(
      "Unsupported typography registry schema version",
    );
  });

  it("parses and caches typography registry", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_TYPOGRAPHY_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedTypographyRegistryUrls()[0];
    const first = await fetchTypographyRegistry(trusted);
    const second = await fetchTypographyRegistry(trusted);

    expect(first.schema_version).toBe(1);
    expect(first.typographies).toHaveLength(1);
    expect(first.typographies[0]?.id).toBe("typography.editorial-serif");
    expect(first.typographies[0]?.typography.settings.fontFamily).toBe("serif");
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
