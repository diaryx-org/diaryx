import type { BundleRegistryEntry } from "$lib/marketplace/types";
import {
  type RegistryPlugin,
} from "$lib/plugins/pluginRegistry";
import {
  inspectPluginWasm,
} from "$lib/plugins/browserPluginManager.svelte";
import {
  verifyRegistryArtifact,
} from "$lib/plugins/pluginInstallService";
import type { PluginPermissions } from "@/models/stores/permissionStore.svelte";

type BundlePluginDependency = BundleRegistryEntry["plugins"][number];

type PersistDefaults = (
  pluginId: string,
  defaults: PluginPermissions,
) => Promise<void>;

interface HydrateOnboardingPluginPermissionDefaultsOptions {
  fetchImpl?: typeof fetch;
  inspectPluginBytes?: typeof inspectPluginWasm;
  verifyArtifact?: typeof verifyRegistryArtifact;
}

const FALLBACK_PERMISSION_DEFAULTS: Partial<Record<string, PluginPermissions>> = {
  // Temporary workaround for the currently published diaryx.publish artifact,
  // which does not expose requested_permissions even though the source manifest does.
  "diaryx.publish": {
    read_files: { include: ["all"], exclude: [] },
    edit_files: { include: ["all"], exclude: [] },
    create_files: { include: ["all"], exclude: [] },
    http_requests: { include: ["unpkg.com"], exclude: [] },
    plugin_storage: { include: ["all"], exclude: [] },
  },
};

function hasPermissionDefaults(
  defaults: PluginPermissions | undefined | null,
): defaults is PluginPermissions {
  if (!defaults) return false;
  return Object.values(defaults).some((rule) => rule != null);
}

function getRegistryDefaults(plugin: RegistryPlugin): PluginPermissions | undefined {
  const requested = plugin.requested_permissions as { defaults?: PluginPermissions } | null;
  return requested?.defaults;
}

function getFallbackDefaults(pluginId: string): PluginPermissions | undefined {
  return FALLBACK_PERMISSION_DEFAULTS[pluginId];
}

async function getManifestDefaults(
  plugin: RegistryPlugin,
  {
    fetchImpl = fetch,
    inspectPluginBytes = inspectPluginWasm,
    verifyArtifact = verifyRegistryArtifact,
  }: HydrateOnboardingPluginPermissionDefaultsOptions,
): Promise<PluginPermissions | undefined> {
  const response = await fetchImpl(plugin.artifact.url);
  if (!response.ok) {
    throw new Error(`Failed to fetch plugin artifact for ${plugin.id}: ${response.status}`);
  }

  const bytes = await response.arrayBuffer();
  await verifyArtifact(bytes, plugin.artifact.sha256);
  const inspected = await inspectPluginBytes(bytes);
  return inspected.requestedPermissions?.defaults;
}

export async function hydrateOnboardingPluginPermissionDefaults(
  dependencies: BundlePluginDependency[],
  registryPlugins: RegistryPlugin[],
  persistDefaults: PersistDefaults,
  options: HydrateOnboardingPluginPermissionDefaultsOptions = {},
): Promise<void> {
  const pluginById = new Map(registryPlugins.map((plugin) => [plugin.id, plugin]));

  for (const dependency of dependencies) {
    const plugin = pluginById.get(dependency.plugin_id);
    if (!plugin) continue;

    let defaults = getRegistryDefaults(plugin);
    if (!hasPermissionDefaults(defaults)) {
      try {
        defaults = await getManifestDefaults(plugin, options);
      } catch (error) {
        console.warn(
          `[onboarding] Failed to inspect requested permissions for ${plugin.id}:`,
          error,
        );
        continue;
      }
    }

    if (!hasPermissionDefaults(defaults)) {
      defaults = getFallbackDefaults(dependency.plugin_id);
    }

    if (!hasPermissionDefaults(defaults)) {
      continue;
    }

    await persistDefaults(dependency.plugin_id, defaults);
  }
}
