import { describe, expect, it } from "vitest";

import { classifyMarketplacePlugins } from "./marketplacePluginClassification";

describe("classifyMarketplacePlugins", () => {
  it("treats a locally installed plugin as a local override even when version matches registry", () => {
    const result = classifyMarketplacePlugins(
      [{
        id: "diaryx.publish" as never,
        name: "Publish",
        version: "1.0.0",
        description: "Ship a site",
        capabilities: [],
        cli: [],
        ui: [],
      }],
      [{
        id: "diaryx.publish",
        name: "Publish",
        version: "1.0.0",
        summary: "Ship a site",
        description: "Ship a site",
        author: "Diaryx",
        license: "MIT",
        artifact: {
          url: "https://example.com/publish.wasm",
          sha256: "abc",
          size: 123,
          published_at: "2026-03-05T00:00:00Z",
        },
        repository: null,
        categories: [],
        tags: [],
        icon: null,
        screenshots: [],
        capabilities: [],
        requested_permissions: null,
      }],
      { "diaryx.publish": "local" },
    );

    expect(result.localOverrides).toHaveLength(1);
    expect(result.localOverrideIds.has("diaryx.publish")).toBe(true);
    expect(result.localPlugins).toEqual([]);
  });

  it("keeps unmanaged IDs in the local plugins section", () => {
    const result = classifyMarketplacePlugins(
      [{
        id: "local.only" as never,
        name: "Local Only",
        version: "0.1.0",
        description: "dev build",
        capabilities: [],
        cli: [],
        ui: [],
      }],
      [],
      {},
    );

    expect(result.localOverrides).toEqual([]);
    expect(result.localPlugins).toHaveLength(1);
    expect(String(result.localPlugins[0]?.id)).toBe("local.only");
  });
});
