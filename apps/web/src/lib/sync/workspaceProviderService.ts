/**
 * Workspace Provider Service
 *
 * Provider operations are plugin-command-backed.
 * The host owns only local workspace registry updates.
 */

import { getBackend, createApi } from "$lib/backend";
import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import type {
  PermissionRule,
  PluginConfig,
  PluginPermissions,
} from "@/models/stores/permissionStore.svelte";
import { inspectPluginWasm } from "$lib/plugins/browserPluginManager.svelte";
import { executeProviderPluginCommand } from "$lib/sync/providerPluginCommands";
import {
  addLocalWorkspace,
  setCurrentWorkspaceId,
  setPluginMetadata,
  createLocalWorkspace,
  getLocalWorkspace,
  getWorkspaceStorageType,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import {
  captureProviderPluginForTransfer,
  installCapturedProviderPlugin,
} from "$lib/sync/browserProviderBootstrap";

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

function toPluginBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
}

function clonePermissionRule(rule: PermissionRule): PermissionRule {
  return {
    include: [...(rule.include ?? [])],
    exclude: [...(rule.exclude ?? [])],
  };
}

async function persistPluginDefaultPermissions(args: {
  api: Api;
  rootIndexPath: string;
  pluginId: string;
  defaults: PluginPermissions;
}): Promise<void> {
  const { api, rootIndexPath, pluginId, defaults } = args;
  const frontmatter = await api.getFrontmatter(rootIndexPath);
  const existingPlugins =
    (frontmatter.plugins as Record<string, PluginConfig> | undefined) ?? {};
  const existingPluginConfig = existingPlugins[pluginId] ?? { permissions: {} };
  const mergedPermissions: PluginPermissions = {
    ...(existingPluginConfig.permissions ?? {}),
  };

  let changed = false;
  for (const [permissionType, requestedRule] of Object.entries(defaults)) {
    if (!requestedRule) continue;
    if (!mergedPermissions[permissionType as keyof PluginPermissions]) {
      mergedPermissions[permissionType as keyof PluginPermissions] =
        clonePermissionRule(requestedRule as PermissionRule) as never;
      changed = true;
    }
  }

  if (!changed) return;

  const nextPlugins: Record<string, PluginConfig> = {
    ...existingPlugins,
    [pluginId]: {
      ...existingPluginConfig,
      permissions: mergedPermissions,
    },
  };

  await api.setFrontmatterProperty(
    rootIndexPath,
    "plugins",
    nextPlugins as unknown as JsonValue,
    rootIndexPath,
  );
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
    const result = await executeProviderPluginCommand<GetProviderStatusResponse>({
      api: client,
      pluginId,
      command: "GetProviderStatus",
    });
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
  const result = await executeProviderPluginCommand<ListRemoteWorkspacesResponse>({
    api: client,
    pluginId,
    command: "ListRemoteWorkspaces",
  });
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
  const localWorkspace = getLocalWorkspace(params.localId);

  onProgress?.({ percent: 8, message: `Starting sync for "${params.name}"...` });

  const response = await executeProviderPluginCommand<LinkWorkspaceResponse>({
    api: client,
    pluginId,
    command: "LinkWorkspace",
    params: {
      local_workspace_id: params.localId,
      name: params.name,
      remote_id: params.remoteId ?? null,
      ...(localWorkspace?.path ? { workspace_root: localWorkspace.path } : {}),
    },
  });

  const remoteId = typeof response?.remote_id === "string" ? response.remote_id : "";
  if (!remoteId) {
    throw new Error("Provider returned an invalid remote workspace ID");
  }

  setPluginMetadata(params.localId, pluginId, {
    remoteWorkspaceId: remoteId,
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

export async function uploadWorkspaceSnapshot(
  pluginId: string,
  params: { remoteId: string; mode?: "replace" | "merge"; includeAttachments?: boolean },
  api?: Api | null,
): Promise<{ filesUploaded: number; snapshotUploaded: boolean }> {
  const client = await resolveApi(api);
  const response = await executeProviderPluginCommand<{
    files_uploaded?: number;
    snapshot_uploaded?: boolean;
  }>({
    api: client,
    pluginId,
    command: "UploadWorkspaceSnapshot",
    params: {
      remote_id: params.remoteId,
      mode: params.mode ?? "replace",
      include_attachments: params.includeAttachments ?? true,
    },
  });

  return {
    filesUploaded: Number(response?.files_uploaded ?? 0),
    snapshotUploaded: !!response?.snapshot_uploaded,
  };
}

export async function unlinkWorkspace(
  pluginId: string,
  localId: string,
  api?: Api | null,
): Promise<void> {
  const client = await resolveApi(api);
  await executeProviderPluginCommand({
    api: client,
    pluginId,
    command: "UnlinkWorkspace",
    params: {
      local_workspace_id: localId,
    },
  });

  setPluginMetadata(localId, pluginId, null);
}

export async function downloadWorkspace(
  pluginId: string,
  params: { remoteId: string; name: string; link?: boolean },
  onProgress?: ProgressCallback,
  api?: Api | null,
): Promise<{ localId: string; filesImported: number }> {
  const client = await resolveApi(api);
  const capturedProviderPlugin = await captureProviderPluginForTransfer(pluginId);
  const workspacePluginDefaults = capturedProviderPlugin
    ? (await inspectPluginWasm(toPluginBuffer(capturedProviderPlugin)))
      .requestedPermissions?.defaults
    : undefined;

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

  await installCapturedProviderPlugin(pluginId, capturedProviderPlugin);

  const workspaceApi = createApi(backend);

  onProgress?.({ percent: 40, message: "Downloading workspace..." });

  const result = await executeProviderPluginCommand<DownloadWorkspaceResponse>({
    api: client,
    pluginId,
    command: "DownloadWorkspace",
    params: {
      workspace_root: workspaceRoot,
      remote_id: params.remoteId,
      link: !!params.link,
    },
  });

  if (workspacePluginDefaults) {
    try {
      const rootIndexPath = await workspaceApi.findRootIndex(workspaceRoot);
      await persistPluginDefaultPermissions({
        api: workspaceApi,
        rootIndexPath,
        pluginId,
        defaults: workspacePluginDefaults,
      });
    } catch (error) {
      console.warn(
        `[workspaceProviderService] Failed to persist default permissions for ${pluginId}:`,
        error,
      );
    }
  }

  if (params.link) {
    setPluginMetadata(localWs.id, pluginId, {
      remoteWorkspaceId: params.remoteId,
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
