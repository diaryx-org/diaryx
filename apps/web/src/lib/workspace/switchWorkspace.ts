import { getBackend, resetBackend } from "$lib/backend";
import { createApi } from "$lib/backend/api";
import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import { setActiveWorkspaceId } from "$lib/auth";
import {
  setCurrentWorkspaceId,
  getWorkspaceStorageType,
  getWorkspaceStoragePluginId,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import { hydrateProviderLinksFromFrontmatter } from "./hydrateProviderLinks";

export interface SwitchWorkspaceOptions {
  onTeardownComplete?: () => void;
  onReady?: () => void;
}

/**
 * Switch the active workspace by resetting and re-initializing the backend.
 * Sync/session semantics are plugin-owned and not handled here.
 */
export async function switchWorkspace(
  workspaceId: string,
  workspaceName: string,
  options?: SwitchWorkspaceOptions,
): Promise<void> {
  options?.onTeardownComplete?.();

  setCurrentWorkspaceId(workspaceId);
  setActiveWorkspaceId(workspaceId);
  resetBackend();

  const backend = await getBackend(
    workspaceId,
    workspaceName,
    getWorkspaceStorageType(workspaceId),
    getWorkspaceStoragePluginId(workspaceId),
  );
  workspaceStore.setBackend(backend);
  const api = createApi(backend);
  await getPluginStore().init(api);
  await hydrateProviderLinksFromFrontmatter(workspaceId, api, backend);

  options?.onReady?.();
}
