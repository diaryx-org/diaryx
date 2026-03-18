import type { PluginManifest } from "$lib/backend/generated";
import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import type { PluginInstallSource } from "$lib/plugins/pluginInstallSource.svelte";

function differsFromRegistry(
  manifest: PluginManifest,
  registry: RegistryPlugin,
): boolean {
  return (
    String(manifest.version ?? "") !== registry.version ||
    String(manifest.name ?? "") !== registry.name ||
    String(manifest.description ?? "") !== registry.description
  );
}

export interface UpdatablePlugin {
  installed: PluginManifest;
  registry: RegistryPlugin;
}

export function classifyMarketplacePlugins(
  manifests: PluginManifest[],
  registryPlugins: RegistryPlugin[],
  installedSources: Record<string, PluginInstallSource>,
): {
  localOverrides: Array<{ installed: PluginManifest; registry: RegistryPlugin }>;
  localOverrideIds: Set<string>;
  localPlugins: PluginManifest[];
  updatable: UpdatablePlugin[];
  updatableIds: Set<string>;
} {
  const registryById = new Map(registryPlugins.map((plugin) => [plugin.id, plugin]));
  const localOverrides: Array<{ installed: PluginManifest; registry: RegistryPlugin }> = [];
  const localPlugins: PluginManifest[] = [];
  const updatable: UpdatablePlugin[] = [];

  for (const manifest of manifests) {
    const pluginId = String(manifest.id);
    const registry = registryById.get(pluginId);
    const installSource = installedSources[pluginId] ?? null;

    if (!registry) {
      localPlugins.push(manifest);
      continue;
    }

    // Plugins explicitly installed from a local file are always local overrides.
    if (installSource === "local") {
      localOverrides.push({ installed: manifest, registry });
      continue;
    }

    // Registry-installed plugins with a different version are "updatable", not
    // local overrides.  This fixes the false "local override" badge that appeared
    // whenever the registry shipped a newer release.
    if (installSource === "registry") {
      if (String(manifest.version ?? "") !== registry.version) {
        updatable.push({ installed: manifest, registry });
      }
      // If versions match the plugin is simply "installed" – no special bucket.
      continue;
    }

    // No recorded install source (legacy installs).  Fall back to the previous
    // heuristic: if the metadata differs the plugin is treated as a local
    // override so the user can reconcile.
    if (differsFromRegistry(manifest, registry)) {
      localOverrides.push({ installed: manifest, registry });
    }
  }

  localOverrides.sort((a, b) =>
    String(a.installed.name ?? a.installed.id).localeCompare(
      String(b.installed.name ?? b.installed.id),
    ),
  );
  localPlugins.sort((a, b) =>
    String(a.name ?? a.id).localeCompare(String(b.name ?? b.id)),
  );
  updatable.sort((a, b) =>
    String(a.installed.name ?? a.installed.id).localeCompare(
      String(b.installed.name ?? b.installed.id),
    ),
  );

  return {
    localOverrides,
    localOverrideIds: new Set(localOverrides.map(({ installed }) => String(installed.id))),
    localPlugins,
    updatable,
    updatableIds: new Set(updatable.map(({ installed }) => String(installed.id))),
  };
}
