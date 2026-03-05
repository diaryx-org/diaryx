import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearBundleRegistryCache,
  fetchBundleRegistry,
  getTrustedBundleRegistryUrls,
  normalizeBundleRegistryEntry,
} from "./bundleRegistry";

const VALID_BUNDLE_REGISTRY_MD = `---
schema_version: 1
generated_at: "2026-03-04T00:00:00Z"
bundles:
  - id: "bundle.writer-mode"
    name: "Writer Mode"
    version: "1.0.0"
    summary: "Focused writing bundle"
    description: "Warm theme + readable typography + sync plugin"
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["writing"]
    tags: ["focus", "warm"]
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/bundles/writer-mode.json"
      sha256: "abc"
      size: 456
      published_at: "2026-03-04T00:00:00Z"
    theme_id: "theme.citrus"
    typography_id: "typography.editorial-serif"
    typography:
      fontFamily: "serif"
      baseFontSize: 18
      lineHeight: 1.8
      contentWidth: "narrow"
    plugins:
      - plugin_id: "diaryx.sync"
        required: true
        enable: true
      - plugin_id: "diaryx.publish"
---
# Bundle registry
`;

afterEach(() => {
  clearBundleRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("bundleRegistry", () => {
  it("rejects untrusted URLs", async () => {
    await expect(
      fetchBundleRegistry("https://example.com/bundles/registry.md"),
    ).rejects.toThrow("Untrusted bundle registry URL");
  });

  it("fails on unsupported schema", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "---\nschema_version: 2\ngenerated_at: \"\"\nbundles: []\n---\n",
      }),
    );

    const trusted = getTrustedBundleRegistryUrls()[0];
    await expect(fetchBundleRegistry(trusted)).rejects.toThrow(
      "Unsupported bundle registry schema version",
    );
  });

  it("parses and caches bundle registry", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_BUNDLE_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedBundleRegistryUrls()[0];
    const first = await fetchBundleRegistry(trusted);
    const second = await fetchBundleRegistry(trusted);

    expect(first.schema_version).toBe(1);
    expect(first.bundles).toHaveLength(1);
    expect(first.bundles[0]?.id).toBe("bundle.writer-mode");
    expect(first.bundles[0]?.typography_id).toBe("typography.editorial-serif");
    expect(first.bundles[0]?.plugins[1]).toMatchObject({
      plugin_id: "diaryx.publish",
      required: true,
      enable: true,
    });
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("validates dependency fields", () => {
    expect(() =>
      normalizeBundleRegistryEntry({
        id: "bundle.invalid",
        name: "Invalid",
        version: "1.0.0",
        summary: "Bad",
        description: "Bad",
        author: "Test",
        license: "MIT",
        theme_id: "theme.default",
        plugins: [{ plugin_id: "diaryx.sync", required: "yes" }],
      }),
    ).toThrow("plugin.required");
  });
});
