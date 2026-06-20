/**
 * Workspace config store for managing workspace-level configuration.
 * Uses Svelte 5 runes for reactive state management.
 * Persists configuration to the workspace root index frontmatter.
 */

import { getBackend } from "../backend";
import { createApi } from "../backend/api";
import type { WorkspaceConfig } from "../backend/generated/WorkspaceConfig";
import {
  runPluginUpdateConfigFlow,
  savePluginDeclarativeConfig,
} from "$lib/plugins/configUpdateFlow";
import type { YamlValue } from "../backend/generated/YamlValue";

/**
 * Creates reactive workspace config state with backend persistence.
 */
export function createWorkspaceConfigStore() {
  let config = $state<WorkspaceConfig | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let rootIndexPath = $state<string | null>(null);

  /**
   * Load workspace config from the root index frontmatter.
   */
  async function load(workspaceRootIndex: string) {
    loading = true;
    error = null;
    rootIndexPath = workspaceRootIndex;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      config = await api.getWorkspaceConfig(workspaceRootIndex);
    } catch (e) {
      console.error("[WorkspaceConfigStore] Failed to load config:", e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  /**
   * Set a single workspace config field and persist to the root index.
   */
  async function setField(field: string, value: string) {
    if (!rootIndexPath) {
      error = "No workspace root index loaded";
      return;
    }

    error = null;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      await api.setWorkspaceConfig(rootIndexPath, field, value);
      await runPluginUpdateConfigFlow({
        pluginId: "diaryx.daily",
        api,
        workspacePath: rootIndexPath,
        params: {
          source: "workspace_config",
          field,
          value,
          root_index_path: rootIndexPath,
        },
      });

      // Update local state by re-reading the full config
      // (backend may normalize/validate the value)
      config = await api.getWorkspaceConfig(rootIndexPath);
    } catch (e) {
      console.error("[WorkspaceConfigStore] Failed to set field:", e);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /**
   * Save a plugin's declarative config to `plugins.<id>.config` (for plugins
   * that opt into host-managed config, e.g. the daily plugin). Surfaces any
   * permission request for approval, then re-reads the full workspace config.
   */
  async function savePluginConfig(pluginId: string, pluginConfig: YamlValue) {
    if (!rootIndexPath) {
      error = "No workspace root index loaded";
      return;
    }

    error = null;

    try {
      const backend = await getBackend();
      const api = createApi(backend);
      await savePluginDeclarativeConfig({
        pluginId,
        config: pluginConfig,
        api,
        workspacePath: rootIndexPath,
      });
      config = await api.getWorkspaceConfig(rootIndexPath);
    } catch (e) {
      console.error("[WorkspaceConfigStore] Failed to set plugin config:", e);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  return {
    get config() {
      return config;
    },
    get loading() {
      return loading;
    },
    get error() {
      return error;
    },
    get rootIndexPath() {
      return rootIndexPath;
    },
    load,
    setField,
    savePluginConfig,
  };
}

/**
 * Singleton instance for shared workspace config state across components.
 */
let sharedStore: ReturnType<typeof createWorkspaceConfigStore> | null = null;

export function getWorkspaceConfigStore() {
  if (typeof window === "undefined") {
    // SSR fallback
    return {
      get config() {
        return null as WorkspaceConfig | null;
      },
      get loading() {
        return false;
      },
      get error() {
        return null as string | null;
      },
      get rootIndexPath() {
        return null as string | null;
      },
      load: async () => {},
      setField: async () => {},
      savePluginConfig: async () => {},
    };
  }

  if (!sharedStore) {
    sharedStore = createWorkspaceConfigStore();
  }
  return sharedStore;
}
