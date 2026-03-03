/**
 * Plugin Registry — fetches the CDN-hosted plugin catalog.
 *
 * The registry lists all official (and eventually third-party) plugins
 * available for installation.
 */

export interface RegistryPlugin {
  id: string;
  name: string;
  description: string;
  version: string;
  wasmUrl: string;
}

export interface PluginRegistry {
  version: number;
  plugins: RegistryPlugin[];
}

const REGISTRY_URL = "https://cdn.diaryx.org/plugins/plugins.json";

let cachedRegistry: PluginRegistry | null = null;

export async function fetchPluginRegistry(): Promise<PluginRegistry> {
  if (cachedRegistry) return cachedRegistry;
  const resp = await fetch(REGISTRY_URL);
  if (!resp.ok) throw new Error(`Registry fetch failed: ${resp.status}`);
  cachedRegistry = await resp.json();
  return cachedRegistry!;
}

export function clearRegistryCache(): void {
  cachedRegistry = null;
}
