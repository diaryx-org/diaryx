export type PluginInstallSource = "local" | "registry";

const STORAGE_KEY = "diaryx-plugin-install-source";

let installSourcesState: Record<string, PluginInstallSource> = $state(loadSources());

function loadSources(): Record<string, PluginInstallSource> {
  if (typeof localStorage === "undefined") return {};

  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Record<string, PluginInstallSource>;
  } catch {
    return {};
  }
}

function saveSources(sources: Record<string, PluginInstallSource>): void {
  if (typeof localStorage !== "undefined") {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(sources));
    } catch {
      // Ignore storage write failures.
    }
  }

  installSourcesState = { ...sources };
}

export function getInstalledPluginSources(): Record<string, PluginInstallSource> {
  return installSourcesState;
}

export function getInstalledPluginSource(pluginId: string): PluginInstallSource | null {
  return installSourcesState[pluginId] ?? null;
}

export function setInstalledPluginSource(
  pluginId: string,
  source: PluginInstallSource,
): void {
  saveSources({
    ...installSourcesState,
    [pluginId]: source,
  });
}

export function clearInstalledPluginSource(pluginId: string): void {
  const next = { ...installSourcesState };
  delete next[pluginId];
  saveSources(next);
}
