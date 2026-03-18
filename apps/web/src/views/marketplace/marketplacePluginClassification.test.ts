import { describe, expect, it } from "vitest";

import { classifyMarketplacePlugins } from "./marketplacePluginClassification";

function makeManifest(overrides: Partial<{ id: string; name: string; version: string; description: string }> = {}) {
  return {
    id: (overrides.id ?? "diaryx.publish") as never,
    name: overrides.name ?? "Publish",
    version: overrides.version ?? "1.0.0",
    description: overrides.description ?? "Ship a site",
    capabilities: [],
    cli: [],
    ui: [],
  };
}

function makeRegistryPlugin(overrides: Partial<{ id: string; name: string; version: string; description: string }> = {}) {
  return {
    id: overrides.id ?? "diaryx.publish",
    name: overrides.name ?? "Publish",
    version: overrides.version ?? "1.0.0",
    summary: "Ship a site",
    description: overrides.description ?? "Ship a site",
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
  };
}

describe("classifyMarketplacePlugins", () => {
  it("treats a locally installed plugin as a local override even when version matches registry", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest()],
      [makeRegistryPlugin()],
      { "diaryx.publish": "local" },
    );

    expect(result.localOverrides).toHaveLength(1);
    expect(result.localOverrideIds.has("diaryx.publish")).toBe(true);
    expect(result.localPlugins).toEqual([]);
    expect(result.updatable).toEqual([]);
  });

  it("keeps unmanaged IDs in the local plugins section", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest({ id: "local.only", name: "Local Only", version: "0.1.0", description: "dev build" })],
      [],
      {},
    );

    expect(result.localOverrides).toEqual([]);
    expect(result.localPlugins).toHaveLength(1);
    expect(String(result.localPlugins[0]?.id)).toBe("local.only");
    expect(result.updatable).toEqual([]);
  });

  it("classifies a registry-installed plugin with outdated version as updatable, not a local override", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest({ version: "1.0.0" })],
      [makeRegistryPlugin({ version: "1.1.0" })],
      { "diaryx.publish": "registry" },
    );

    expect(result.localOverrides).toEqual([]);
    expect(result.localOverrideIds.size).toBe(0);
    expect(result.updatable).toHaveLength(1);
    expect(result.updatableIds.has("diaryx.publish")).toBe(true);
    expect(result.updatable[0]!.installed.version).toBe("1.0.0");
    expect(result.updatable[0]!.registry.version).toBe("1.1.0");
  });

  it("does not mark a registry-installed plugin as updatable when versions match", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest({ version: "1.0.0" })],
      [makeRegistryPlugin({ version: "1.0.0" })],
      { "diaryx.publish": "registry" },
    );

    expect(result.localOverrides).toEqual([]);
    expect(result.updatable).toEqual([]);
    expect(result.localPlugins).toEqual([]);
  });

  it("falls back to differsFromRegistry for legacy installs with no source", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest({ version: "0.9.0" })],
      [makeRegistryPlugin({ version: "1.0.0" })],
      {},
    );

    // No recorded source → legacy heuristic → local override
    expect(result.localOverrides).toHaveLength(1);
    expect(result.updatable).toEqual([]);
  });

  it("does not treat a legacy install as a local override when metadata matches", () => {
    const result = classifyMarketplacePlugins(
      [makeManifest()],
      [makeRegistryPlugin()],
      {},
    );

    expect(result.localOverrides).toEqual([]);
    expect(result.updatable).toEqual([]);
    expect(result.localPlugins).toEqual([]);
  });
});
