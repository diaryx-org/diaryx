import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearRegistryCache,
  fetchPluginRegistry,
  getTrustedRegistryUrls,
} from "./pluginRegistry";

const VALID_REGISTRY = {
  schemaVersion: 2,
  generatedAt: "2026-03-03T00:00:00Z",
  plugins: [
    {
      id: "diaryx.sync",
      name: "Sync",
      version: "1.2.3",
      summary: "Realtime sync",
      description: "Real-time CRDT sync across devices",
      creator: "Diaryx Team",
      license: "PolyForm Shield 1.0.0",
      artifact: {
        wasmUrl: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc.wasm",
        sha256: "abc",
        sizeBytes: 123,
        publishedAt: "2026-03-03T00:00:00Z",
      },
      source: {
        kind: "internal",
        repositoryUrl: "https://github.com/diaryx-org/diaryx",
        registryId: "diaryx-official",
      },
      homepage: null,
      documentationUrl: null,
      changelogUrl: null,
      categories: ["sync"],
      tags: ["crdt"],
      iconUrl: null,
      screenshots: [],
      capabilities: ["sync_transport"],
      requestedPermissions: null,
    },
  ],
};

afterEach(() => {
  clearRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("pluginRegistry v2", () => {
  it("rejects untrusted registry URLs", async () => {
    await expect(fetchPluginRegistry("https://example.com/registry.json")).rejects.toThrow(
      "Untrusted plugin registry URL",
    );
  });

  it("fails fast on non-v2 schema", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ schemaVersion: 1, generatedAt: "", plugins: [] }),
      }),
    );

    const trusted = getTrustedRegistryUrls()[0];
    await expect(fetchPluginRegistry(trusted)).rejects.toThrow(
      "Unsupported plugin registry schema version",
    );
  });

  it("parses registry-v2 and caches results", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => VALID_REGISTRY,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedRegistryUrls()[0];
    const first = await fetchPluginRegistry(trusted);
    const second = await fetchPluginRegistry(trusted);

    expect(first.schemaVersion).toBe(2);
    expect(first.plugins[0]?.id).toBe("diaryx.sync");
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
