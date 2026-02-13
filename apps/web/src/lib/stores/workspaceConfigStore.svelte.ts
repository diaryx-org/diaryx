/**
 * Workspace config store for managing workspace-level configuration.
 * Uses Svelte 5 runes for reactive state management.
 * Persists configuration to the workspace root index frontmatter.
 */

import { getBackend } from "../backend";
import { createApi } from "../backend/api";
import type { WorkspaceConfig } from "../backend/generated/WorkspaceConfig";

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

      // Update local state by re-reading the full config
      // (backend may normalize/validate the value)
      config = await api.getWorkspaceConfig(rootIndexPath);
    } catch (e) {
      console.error("[WorkspaceConfigStore] Failed to set field:", e);
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
    };
  }

  if (!sharedStore) {
    sharedStore = createWorkspaceConfigStore();
  }
  return sharedStore;
}
