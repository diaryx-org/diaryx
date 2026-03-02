/**
 * Workspace Provider Service
 *
 * Provider operations are plugin-command-backed.
 * The host owns only local workspace registry updates.
 */

import { getBackend, createApi } from "$lib/backend";
import type { Api } from "$lib/backend/api";
import {
  addLocalWorkspace,
  setCurrentWorkspaceId,
  setPluginMetadata,
  createLocalWorkspace,
  getWorkspaceStorageType,
  getLocalWorkspace,
} from "$lib/storage/localWorkspaceRegistry.svelte";

// ============================================================================
// Types
// ============================================================================

export interface ProviderStatus {
  ready: boolean;
  message?: string;
}

export interface RemoteWorkspace {
  id: string;
  name: string;
}

export type ProgressCallback = (progress: {
  percent: number;
  message: string;
  detail?: string;
}) => void;

interface GetProviderStatusResponse {
  ready: boolean;
  message?: string | null;
}

interface ListRemoteWorkspacesResponse {
  workspaces: RemoteWorkspace[];
}

interface LinkWorkspaceResponse {
  remote_id: string;
  created_remote: boolean;
  snapshot_uploaded: boolean;
}

interface DownloadWorkspaceResponse {
  files_imported: number;
}

async function resolveApi(api?: Api | null): Promise<Api> {
  if (api) return api;
  const backend = await getBackend();
  return createApi(backend);
}

function asRecord(value: unknown): Record<string, unknown> {
  return (value && typeof value === "object") ? (value as Record<string, unknown>) : {};
}

function parseStatus(value: unknown): ProviderStatus {
  const obj = asRecord(value);
  return {
    ready: obj.ready === true,
    message: typeof obj.message === "string" ? obj.message : undefined,
  };
}

function parseRemoteWorkspaces(value: unknown): RemoteWorkspace[] {
  const obj = asRecord(value);
  const list = Array.isArray(obj.workspaces) ? obj.workspaces : [];
  return list
    .map((item) => {
      const row = asRecord(item);
      const id = typeof row.id === "string" ? row.id : "";
      const name = typeof row.name === "string" ? row.name : "";
      if (!id || !name) return null;
      return { id, name };
    })
    .filter((w): w is RemoteWorkspace => !!w);
}

// ============================================================================
// Query functions
// ============================================================================

export async function getProviderStatus(
  pluginId: string,
  api?: Api | null,
): Promise<ProviderStatus> {
  try {
    const client = await resolveApi(api);
    const result = (await client.executePluginCommand(pluginId, "GetProviderStatus", {
      provider_id: pluginId,
    })) as unknown as GetProviderStatusResponse;
    return parseStatus(result);
  } catch (e) {
    return {
      ready: false,
      message: e instanceof Error ? e.message : String(e),
    };
  }
}

export async function listRemoteWorkspaces(
  pluginId: string,
  api?: Api | null,
): Promise<RemoteWorkspace[]> {
  const client = await resolveApi(api);
  const result = (await client.executePluginCommand(pluginId, "ListRemoteWorkspaces", {
    provider_id: pluginId,
  })) as unknown as ListRemoteWorkspacesResponse;
  return parseRemoteWorkspaces(result);
}

export async function listUnlinkedRemoteWorkspaces(
  pluginId: string,
  localWorkspaceServerIds: Set<string>,
  api?: Api | null,
): Promise<RemoteWorkspace[]> {
  const all = await listRemoteWorkspaces(pluginId, api);
  return all.filter((w) => !localWorkspaceServerIds.has(w.id));
}

// ============================================================================
// Mutation functions
// ============================================================================

export async function linkWorkspace(
  pluginId: string,
  params: { localId: string; name: string; remoteId?: string },
  onProgress?: ProgressCallback,
  api?: Api | null,
): Promise<{ remoteId: string; createdRemote: boolean; snapshotUploaded: boolean }> {
  const client = await resolveApi(api);
  const local = getLocalWorkspace(params.localId);

  onProgress?.({ percent: 8, message: `Starting sync for "${params.name}"...` });

  const response = (await client.executePluginCommand(pluginId, "LinkWorkspace", {
    provider_id: pluginId,
    local_workspace_id: params.localId,
    name: params.name,
    workspace_root: local?.path ?? "",
    remote_id: params.remoteId ?? null,
  })) as unknown as LinkWorkspaceResponse;

  const remoteId = typeof response?.remote_id === "string" ? response.remote_id : "";
  if (!remoteId) {
    throw new Error("Provider returned an invalid remote workspace ID");
  }

  setPluginMetadata(params.localId, "sync", {
    serverId: remoteId,
    syncEnabled: true,
  });

  onProgress?.({ percent: 100, message: "Sync enabled." });

  return {
    remoteId,
    createdRemote: !!response?.created_remote,
    snapshotUploaded: !!response?.snapshot_uploaded,
  };
}

export async function unlinkWorkspace(
  pluginId: string,
  localId: string,
  api?: Api | null,
): Promise<void> {
  const client = await resolveApi(api);
  await client.executePluginCommand(pluginId, "UnlinkWorkspace", {
    provider_id: pluginId,
    local_workspace_id: localId,
  });

  setPluginMetadata(localId, "sync", null);
}

export async function downloadWorkspace(
  pluginId: string,
  params: { remoteId: string; name: string; link?: boolean },
  onProgress?: ProgressCallback,
  api?: Api | null,
): Promise<{ localId: string; filesImported: number }> {
  const client = await resolveApi(api);

  onProgress?.({ percent: 10, message: "Creating local workspace..." });

  const localWs = createLocalWorkspace(params.name);
  addLocalWorkspace({ id: localWs.id, name: params.name });
  setCurrentWorkspaceId(localWs.id);

  const storageType = getWorkspaceStorageType(localWs.id);
  const backend = await getBackend(localWs.id, params.name, storageType);
  const workspaceRoot = backend
    .getWorkspacePath()
    .replace(/\/index\.md$/, "")
    .replace(/\/README\.md$/, "");

  if (workspaceRoot && workspaceRoot !== ".") {
    addLocalWorkspace({ id: localWs.id, name: params.name, path: workspaceRoot });
  }

  onProgress?.({ percent: 40, message: "Downloading workspace..." });

  const result = (await client.executePluginCommand(pluginId, "DownloadWorkspace", {
    provider_id: pluginId,
    remote_id: params.remoteId,
    workspace_root: workspaceRoot,
    link: !!params.link,
  })) as unknown as DownloadWorkspaceResponse;

  if (params.link) {
    setPluginMetadata(localWs.id, "sync", {
      serverId: params.remoteId,
      syncEnabled: true,
    });
  }

  onProgress?.({ percent: 100, message: "Done." });

  return {
    localId: localWs.id,
    filesImported: Number(result?.files_imported ?? 0),
  };
}
