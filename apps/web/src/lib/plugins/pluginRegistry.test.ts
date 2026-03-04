import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearRegistryCache,
  fetchPluginRegistry,
  getTrustedRegistryUrls,
} from "./pluginRegistry";

const VALID_REGISTRY_MD = `---
title: "Diaryx Plugin Registry"
description: "Official plugin directory"
generated_at: "2026-03-03T00:00:00Z"
schema_version: 2
plugins:
  - id: "diaryx.sync"
    name: "Sync"
    version: "1.2.3"
    summary: "Realtime sync"
    description: "Real-time CRDT sync across devices"
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    artifact:
      url: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc.wasm"
      sha256: "abc"
      size: 123
      published_at: "2026-03-03T00:00:00Z"
    repository: "https://github.com/diaryx-org/diaryx"
    categories: ["sync"]
    tags: ["crdt"]
    icon: null
    screenshots: []
    capabilities: ["sync_transport"]
    requested_permissions: null
---
# Diaryx Plugin Registry
Browse and install plugins for Diaryx.
`;

afterEach(() => {
  clearRegistryCache();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("pluginRegistry", () => {
  it("rejects untrusted registry URLs", async () => {
    await expect(fetchPluginRegistry("https://example.com/registry.md")).rejects.toThrow(
      "Untrusted plugin registry URL",
    );
  });

  it("fails fast on non-v2 schema", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        text: async () => "---\nschema_version: 1\ngenerated_at: \"\"\nplugins: []\n---\n",
      }),
    );

    const trusted = getTrustedRegistryUrls()[0];
    await expect(fetchPluginRegistry(trusted)).rejects.toThrow(
      "Unsupported plugin registry schema version",
    );
  });

  it("parses registry markdown and caches results", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () => VALID_REGISTRY_MD,
    });
    vi.stubGlobal("fetch", fetchMock);

    const trusted = getTrustedRegistryUrls()[0];
    const first = await fetchPluginRegistry(trusted);
    const second = await fetchPluginRegistry(trusted);

    expect(first.schema_version).toBe(2);
    expect(first.plugins[0]?.id).toBe("diaryx.sync");
    expect(first.plugins[0]?.author).toBe("Diaryx Team");
    expect(first.plugins[0]?.artifact.url).toContain("abc.wasm");
    expect(first.plugins[0]?.artifact.size).toBe(123);
    expect(second).toBe(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
