import { describe, expect, it, vi } from "vitest";

import type { BundleRegistryEntry } from "$lib/marketplace/types";
import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import type { PluginPermissions } from "@/models/stores/permissionStore.svelte";

import { hydrateOnboardingPluginPermissionDefaults } from "./onboardingPluginPermissions";

function makeBundleDependency(pluginId: string): BundleRegistryEntry["plugins"][number] {
  return {
    plugin_id: pluginId,
    required: true,
    enable: true,
  };
}

function makeDefaults(): PluginPermissions {
  return {
    plugin_storage: {
      include: ["all"],
      exclude: [],
    },
  };
}

function makeRegistryPlugin(
  pluginId: string,
  requested_permissions: RegistryPlugin["requested_permissions"],
): RegistryPlugin {
  return {
    id: pluginId,
    name: pluginId,
    version: "0.1.0",
    summary: pluginId,
    description: pluginId,
    author: "Diaryx",
    license: "PolyForm Shield 1.0.0",
    repository: null,
    categories: [],
    tags: [],
    icon: null,
    screenshots: [],
    capabilities: [],
    requested_permissions,
    artifact: {
      url: `https://app.diaryx.org/cdn/plugins/${pluginId}.wasm`,
      sha256: "abc123",
      size: 123,
      published_at: "2026-03-10T00:00:00Z",
    },
  };
}

describe("hydrateOnboardingPluginPermissionDefaults", () => {
  it("uses registry requested_permissions defaults when present", async () => {
    const defaults = makeDefaults();
    const persistDefaults = vi.fn().mockResolvedValue(undefined);
    const fetchImpl = vi.fn();

    await hydrateOnboardingPluginPermissionDefaults(
      [makeBundleDependency("diaryx.ai")],
      [makeRegistryPlugin("diaryx.ai", { defaults })],
      persistDefaults,
      { fetchImpl: fetchImpl as typeof fetch },
    );

    expect(fetchImpl).not.toHaveBeenCalled();
    expect(persistDefaults).toHaveBeenCalledWith("diaryx.ai", defaults);
  });

  it("falls back to inspecting the plugin artifact when registry metadata omits defaults", async () => {
    const defaults = makeDefaults();
    const persistDefaults = vi.fn().mockResolvedValue(undefined);
    const verifyArtifact = vi.fn().mockResolvedValue(undefined);
    const inspectPluginBytes = vi.fn().mockResolvedValue({
      requestedPermissions: { defaults },
    });
    const arrayBuffer = new ArrayBuffer(8);
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      arrayBuffer: vi.fn().mockResolvedValue(arrayBuffer),
    } satisfies Pick<Response, "ok" | "status" | "arrayBuffer">);

    await hydrateOnboardingPluginPermissionDefaults(
      [makeBundleDependency("diaryx.sync")],
      [makeRegistryPlugin("diaryx.sync", null)],
      persistDefaults,
      {
        fetchImpl: fetchImpl as typeof fetch,
        verifyArtifact,
        inspectPluginBytes,
      },
    );

    expect(fetchImpl).toHaveBeenCalledWith("https://app.diaryx.org/cdn/plugins/diaryx.sync.wasm");
    expect(verifyArtifact).toHaveBeenCalledWith(arrayBuffer, "abc123");
    expect(inspectPluginBytes).toHaveBeenCalledWith(arrayBuffer);
    expect(persistDefaults).toHaveBeenCalledWith("diaryx.sync", defaults);
  });

  it("skips persistence when neither registry metadata nor manifest inspection yields defaults", async () => {
    const persistDefaults = vi.fn().mockResolvedValue(undefined);
    const inspectPluginBytes = vi.fn().mockResolvedValue({
      requestedPermissions: undefined,
    });
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      arrayBuffer: vi.fn().mockResolvedValue(new ArrayBuffer(8)),
    } satisfies Pick<Response, "ok" | "status" | "arrayBuffer">);

    await hydrateOnboardingPluginPermissionDefaults(
      [makeBundleDependency("diaryx.ai")],
      [makeRegistryPlugin("diaryx.ai", null)],
      persistDefaults,
      {
        fetchImpl: fetchImpl as typeof fetch,
        verifyArtifact: vi.fn().mockResolvedValue(undefined),
        inspectPluginBytes,
      },
    );

    expect(persistDefaults).not.toHaveBeenCalled();
  });

  it("persists defaults deterministically across multiple bundle plugins", async () => {
    const persistedByPlugin = new Map<string, PluginPermissions>();
    const persistDefaults = vi.fn().mockImplementation(async (pluginId: string, defaults: PluginPermissions) => {
      persistedByPlugin.set(pluginId, defaults);
    });

    await hydrateOnboardingPluginPermissionDefaults(
      [
        makeBundleDependency("diaryx.import"),
        makeBundleDependency("diaryx.publish"),
        makeBundleDependency("diaryx.templating"),
      ],
      [
        makeRegistryPlugin("diaryx.import", { defaults: makeDefaults() }),
        makeRegistryPlugin("diaryx.publish", { defaults: makeDefaults() }),
        makeRegistryPlugin("diaryx.templating", { defaults: makeDefaults() }),
      ],
      persistDefaults,
    );

    expect(persistDefaults).toHaveBeenCalledTimes(3);
    expect(Array.from(persistedByPlugin.keys())).toEqual([
      "diaryx.import",
      "diaryx.publish",
      "diaryx.templating",
    ]);
  });

  it("falls back to curated defaults for publish when registry and artifact metadata are both missing", async () => {
    const persistDefaults = vi.fn().mockResolvedValue(undefined);
    const inspectPluginBytes = vi.fn().mockResolvedValue({
      requestedPermissions: undefined,
    });
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      arrayBuffer: vi.fn().mockResolvedValue(new ArrayBuffer(8)),
    } satisfies Pick<Response, "ok" | "status" | "arrayBuffer">);

    await hydrateOnboardingPluginPermissionDefaults(
      [makeBundleDependency("diaryx.publish")],
      [makeRegistryPlugin("diaryx.publish", null)],
      persistDefaults,
      {
        fetchImpl: fetchImpl as typeof fetch,
        verifyArtifact: vi.fn().mockResolvedValue(undefined),
        inspectPluginBytes,
      },
    );

    expect(persistDefaults).toHaveBeenCalledWith("diaryx.publish", {
      read_files: { include: ["all"], exclude: [] },
      edit_files: { include: ["all"], exclude: [] },
      create_files: { include: ["all"], exclude: [] },
      http_requests: { include: ["unpkg.com"], exclude: [] },
      plugin_storage: { include: ["all"], exclude: [] },
    });
  });
});
