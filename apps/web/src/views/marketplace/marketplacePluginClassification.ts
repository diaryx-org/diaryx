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

export function classifyMarketplacePlugins(
  manifests: PluginManifest[],
  registryPlugins: RegistryPlugin[],
  installedSources: Record<string, PluginInstallSource>,
): {
  localOverrides: Array<{ installed: PluginManifest; registry: RegistryPlugin }>;
  localOverrideIds: Set<string>;
  localPlugins: PluginManifest[];
} {
  const registryById = new Map(registryPlugins.map((plugin) => [plugin.id, plugin]));
  const localOverrides: Array<{ installed: PluginManifest; registry: RegistryPlugin }> = [];
  const localPlugins: PluginManifest[] = [];

  for (const manifest of manifests) {
    const pluginId = String(manifest.id);
    const registry = registryById.get(pluginId);
    const installSource = installedSources[pluginId] ?? null;

    if (!registry) {
      localPlugins.push(manifest);
      continue;
    }

    if (installSource === "local" || differsFromRegistry(manifest, registry)) {
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

  return {
    localOverrides,
    localOverrideIds: new Set(localOverrides.map(({ installed }) => String(installed.id))),
    localPlugins,
  };
}
