/**
 * Workspace Provider Service
 *
 * Provider operations are plugin-command-backed.
 * The host owns only local workspace registry updates.
 */

import { getBackend, createApi, isTauri } from "$lib/backend";
import type { Api } from "$lib/backend/api";
import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";
import { resolveStorageType } from "$lib/backend/storageType";
import type {
  PermissionRule,
  PluginConfig,
  PluginPermissions,
} from "@/models/stores/permissionStore.svelte";
import { inspectPluginWasm, loadAllPlugins, loadPluginWithCustomInit } from "$lib/plugins/browserPluginManager.svelte";
import { executeProviderPluginCommand } from "$lib/sync/providerPluginCommands";
import { isBuiltinProvider } from "$lib/sync/builtinProviders";
import {
  addLocalWorkspace,
  setCurrentWorkspaceId,
  setPluginMetadata,
  createLocalWorkspace,
  getLocalWorkspace,
  getLocalWorkspaces,
  getWorkspaceStorageType,
  removeLocalWorkspace,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import { resetBackend } from "$lib/backend";
import {
  captureProviderPluginForTransfer,
  installCapturedProviderPlugin,
} from "$lib/sync/browserProviderBootstrap";
import { listUserWorkspaceNamespaces } from "$lib/auth";

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

export type ExistingWorkspaceLinkPolicy = "link_only" | "upload_local";

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
  /** Files skipped because the local manifest already had them (resumed download). */
  files_resumed_skip?: number;
  /** Non-markdown file keys deferred for background download. */
  deferred_files?: string[];
}

/**
 * Handle returned alongside a download promise so callers can cancel it.
 *
 * Cancellation is cooperative: the WASM plugin polls the host between batches
 * and, on observing the flag, saves whatever progress it has and returns an
 * error. Re-invoking `downloadWorkspace` with the same `remote_id` resumes
 * from the persisted manifest.
 */
export interface DownloadCancelHandle {
  /** Flip the cancellation flag observed by the plugin's next poll. */
  cancel(): void;
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
    ...(rule.quota_bytes !== undefined ? { quota_bytes: rule.quota_bytes } : {}),
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
  if (pluginId === "diaryx.sync") {
    const namespaces = await listUserWorkspaceNamespaces();
    return (namespaces ?? [])
      .filter((entry) => (entry.metadata?.provider ?? "diaryx.sync") === pluginId)
      .map((entry) => ({
        id: entry.id,
        name:
          typeof entry.metadata?.name === "string" &&
          entry.metadata.name.trim().length > 0
            ? entry.metadata.name
            : entry.id,
      }));
  }

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

export async function attachExistingLocalWorkspaceToRemote(
  pluginId: string,
  params: {
    remoteId: string;
    remoteName: string;
    localPath: string;
    localId?: string;
    policy: ExistingWorkspaceLinkPolicy;
  },
  onProgress?: ProgressCallback,
  api?: Api | null,
): Promise<{
  localId: string;
  localName: string;
  remoteId: string;
  snapshotUploaded: boolean;
}> {
  const normalizedPath = params.localPath.trim();
  if (!normalizedPath) {
    throw new Error("Local workspace path is required.");
  }

  const existingWorkspace = params.localId
    ? getLocalWorkspace(params.localId)
    : getLocalWorkspaces().find((workspace) => workspace.path === normalizedPath);
  const localWorkspace = existingWorkspace ?? createLocalWorkspace(
    params.remoteName,
    undefined,
    normalizedPath,
  );
  const localName = existingWorkspace?.name ?? params.remoteName;

  addLocalWorkspace({
    id: localWorkspace.id,
    name: localName,
    path: normalizedPath,
  });

  if (params.policy === "link_only") {
    onProgress?.({ percent: 20, message: `Linking "${localName}"...` });
    setPluginMetadata(localWorkspace.id, pluginId, {
      remoteWorkspaceId: params.remoteId,
      serverId: params.remoteId,
      syncEnabled: true,
    });
    onProgress?.({ percent: 100, message: "Workspace linked." });

    return {
      localId: localWorkspace.id,
      localName,
      remoteId: params.remoteId,
      snapshotUploaded: false,
    };
  }

  const linkResult = await linkWorkspace(
    pluginId,
    {
      localId: localWorkspace.id,
      name: localName,
      remoteId: params.remoteId,
    },
    onProgress,
    api,
  );

  let snapshotUploaded = linkResult.snapshotUploaded;
  if (!snapshotUploaded) {
    onProgress?.({ percent: 72, message: `Uploading "${localName}"...` });
    const uploadResult = await uploadWorkspaceSnapshot(
      pluginId,
      { remoteId: linkResult.remoteId },
      api,
    );
    snapshotUploaded = uploadResult.snapshotUploaded;
  }

  return {
    localId: localWorkspace.id,
    localName,
    remoteId: linkResult.remoteId,
    snapshotUploaded,
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

/** Generate an opaque, plugin-scoped cancellation token. */
function generateCancelToken(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `dl:${crypto.randomUUID()}`;
  }
  return `dl:${Date.now()}:${Math.random().toString(36).slice(2)}`;
}

export async function downloadWorkspace(
  pluginId: string,
  params: { remoteId: string; name: string; link?: boolean },
  onProgress?: ProgressCallback,
  _api?: Api | null,
  /** Pre-fetched plugin wasm bytes — used when no existing workspace has the plugin installed. */
  pluginWasm?: Uint8Array | null,
  /** Optional out-param: receives a handle the caller can use to cancel the download. */
  cancelHandleSink?: { handle: DownloadCancelHandle | null },
): Promise<{ localId: string; filesImported: number; filesResumedSkip: number }> {
  if (isBuiltinProvider(pluginId)) {
    onProgress?.({ percent: 15, message: "Restoring workspace..." });

    const currentApi = await resolveApi(_api);
    await executeProviderPluginCommand<DownloadWorkspaceResponse>({
      api: currentApi,
      pluginId,
      command: "DownloadWorkspace",
      params: {
        remote_id: params.remoteId,
        link: !!params.link,
      },
    });

    resetBackend();
    const restoredBackend = await getBackend();
    const restoredApi = createApi(restoredBackend);
    const restoredWorkspaceRoot = restoredBackend
      .getWorkspacePath()
      .replace(/\/index\.md$/, "")
      .replace(/\/README\.md$/, "");

    const existingWorkspace = restoredWorkspaceRoot
      ? getLocalWorkspaces().find((workspace) => workspace.path === restoredWorkspaceRoot)
      : null;
    const localWs = existingWorkspace ?? createLocalWorkspace(
      params.name,
      undefined,
      restoredWorkspaceRoot || undefined,
    );
    addLocalWorkspace({
      id: localWs.id,
      name: params.name,
      ...(restoredWorkspaceRoot ? { path: restoredWorkspaceRoot } : {}),
    });
    setCurrentWorkspaceId(localWs.id);

    if (params.link) {
      setPluginMetadata(localWs.id, pluginId, {
        remoteWorkspaceId: params.remoteId,
        serverId: params.remoteId,
        syncEnabled: true,
      });
    }

    try {
      const rootIndexPath = await restoredApi.resolveWorkspaceRootIndexPath(
        restoredBackend.getWorkspacePath(),
      );
      if (rootIndexPath) {
        const frontmatter = await restoredApi.getFrontmatter(rootIndexPath);
        const title = typeof frontmatter.title === "string" && frontmatter.title.trim().length > 0
          ? frontmatter.title.trim()
          : null;
        if (title && title !== params.name) {
          addLocalWorkspace({
            id: localWs.id,
            name: title,
            ...(restoredWorkspaceRoot ? { path: restoredWorkspaceRoot } : {}),
          });
        }
      }
    } catch (error) {
      console.warn(
        `[workspaceProviderService] Failed to inspect restored built-in workspace ${pluginId}:`,
        error,
      );
    }

    onProgress?.({ percent: 100, message: "Done." });

    return {
      localId: localWs.id,
      filesImported: 0,
      filesResumedSkip: 0,
    };
  }

  const capturedProviderPlugin = pluginWasm ?? await captureProviderPluginForTransfer(pluginId);
  let workspacePluginDefaults: PluginPermissions | undefined;
  if (capturedProviderPlugin) {
    if (isTauri()) {
      const tauriBackend = await getBackend();
      if (tauriBackend.inspectPlugin) {
        const inspected = await tauriBackend.inspectPlugin(capturedProviderPlugin);
        const requestedPermissions = inspected.requestedPermissions as
          | { defaults?: PluginPermissions }
          | undefined;
        workspacePluginDefaults = requestedPermissions?.defaults;
      }
    } else {
      workspacePluginDefaults = (await inspectPluginWasm(toPluginBuffer(capturedProviderPlugin)))
        .requestedPermissions?.defaults;
    }
  }
  const storageType = await resolveStorageType();

  onProgress?.({ percent: 10, message: "Creating local workspace..." });

  // On Tauri, determine a unique workspace directory BEFORE switching backends.
  // Without this, getBackend falls back to the default workspace path
  // (Documents/Diaryx on iOS) and new workspaces share the same directory.
  let tauriWorkspacePath: string | undefined;
  if (isTauri()) {
    const currentBackend = await getBackend();
    const appPaths = currentBackend.getAppPaths?.();
    const docDir = typeof appPaths?.document_dir === 'string' ? appPaths.document_dir : null;
    if (docDir) {
      tauriWorkspacePath = `${docDir}/${params.name}`;
    }
  }

  const localWs = createLocalWorkspace(params.name, storageType, tauriWorkspacePath);
  addLocalWorkspace({ id: localWs.id, name: params.name, ...(tauriWorkspacePath ? { path: tauriWorkspacePath } : {}) });
  setCurrentWorkspaceId(localWs.id);

  const backend = await getBackend(
    localWs.id,
    params.name,
    getWorkspaceStorageType(localWs.id),
    undefined,
    tauriWorkspacePath ? { create: true } : undefined,
  );
  const workspaceRoot = backend
    .getWorkspacePath()
    .replace(/\/index\.md$/, "")
    .replace(/\/README\.md$/, "");

  if (workspaceRoot && workspaceRoot !== ".") {
    addLocalWorkspace({ id: localWs.id, name: params.name, path: workspaceRoot });
  }

  await installCapturedProviderPlugin(pluginId, capturedProviderPlugin);

  if (isTauri()) {
    // On Tauri, install the provider plugin through the native backend.
    // The Tauri plugin system handles loading automatically.
    if (capturedProviderPlugin && backend.installPlugin) {
      await backend.installPlugin(capturedProviderPlugin);
    }
  } else {
    // Load the provider plugin with a minimal init payload.
    // The workspace is empty (no root index yet) so the standard
    // buildBrowserPluginInitPayload would fail trying to resolve the
    // workspace root.  We provide "." (the OPFS root) plus auth/server
    // context so the plugin can connect and download.
    if (capturedProviderPlugin) {
      const buf = capturedProviderPlugin.buffer.slice(
        capturedProviderPlugin.byteOffset,
        capturedProviderPlugin.byteOffset + capturedProviderPlugin.byteLength,
      ) as ArrayBuffer;

      const { getServerUrl, getToken } = await import("$lib/auth");
      await loadPluginWithCustomInit(buf, {
        workspace_root: workspaceRoot || ".",
        workspace_id: localWs.id,
        write_to_disk: true,
        server_url: getServerUrl() ?? null,
        auth_token: getToken() ?? null,
      });
    } else {
      // Fallback: full load (existing workspace may already have content)
      await loadAllPlugins();
    }
  }

  const workspaceApi = createApi(backend);

  onProgress?.({ percent: 40, message: "Downloading workspace..." });

  // Cancellation: hand the caller a handle they can flip to abort the
  // in-flight DownloadWorkspace. The plugin polls between batches and, on
  // observing the flag, saves its manifest and returns. Calling
  // downloadWorkspace again with the same remoteId resumes from where it
  // left off (no need to re-download files we already have on disk).
  const cancelToken = generateCancelToken();
  let cancelled = false;
  if (cancelHandleSink) {
    cancelHandleSink.handle = {
      cancel(): void {
        if (cancelled) return;
        cancelled = true;
        // Best-effort: only the browser loader exposes a cancel registry
        // today. On Tauri this is a no-op; resume-via-restart still works
        // because the manifest checkpoint is persisted on every wave.
        if (!isTauri()) {
          import("$lib/plugins/extismBrowserLoader")
            .then((mod) => {
              mod.cancelPluginOperation(pluginId, cancelToken);
            })
            .catch(() => {
              /* loader unavailable — cancel becomes a no-op */
            });
        }
      },
    };
  }

  let result: DownloadWorkspaceResponse | null;
  try {
    // No host-side timeout: the plugin checkpoints the manifest after every
    // wave, so a stalled or interrupted download never loses more than the
    // last in-flight wave's worth of work. If the user wants to bail, they
    // call cancelHandle.cancel() and the plugin returns gracefully on its
    // next poll. Re-running this function with the same remoteId resumes.
    result = await executeProviderPluginCommand<DownloadWorkspaceResponse>({
      api: workspaceApi,
      pluginId,
      command: "DownloadWorkspace",
      params: {
        workspace_root: workspaceRoot,
        remote_id: params.remoteId,
        link: !!params.link,
        cancel_token: cancelToken,
      },
    });
  } catch (err) {
    const wasCancelled =
      cancelled ||
      (err instanceof Error &&
        /DownloadWorkspace cancelled/i.test(err.message));
    if (wasCancelled) {
      // On user cancel, leave the partial workspace + manifest in place so
      // the user (or a retry) can resume from the checkpoint. We still
      // surface the cancellation to the caller.
      console.info(
        "[workspaceProviderService] DownloadWorkspace cancelled — workspace preserved for resume.",
      );
    } else {
      // Genuine failure: rollback the half-created workspace + reset backend.
      console.error(
        "[workspaceProviderService] DownloadWorkspace failed, rolling back:",
        err,
      );
      removeLocalWorkspace(localWs.id);
      resetBackend();
    }
    throw err;
  } finally {
    // Clear the cancel flag so the token can be reused on retry/resume.
    if (!isTauri()) {
      import("$lib/plugins/extismBrowserLoader")
        .then((mod) => {
          mod.clearPluginOperationCancellation(pluginId, cancelToken);
        })
        .catch(() => {});
    }
  }

  // Persist default plugin permissions — but only for fresh workspaces.
  // When restoring from remote, the downloaded root index already has its
  // own frontmatter/content; writing permission defaults would overwrite it
  // because the WASM backend's in-memory state doesn't reflect the
  // plugin-written files yet.
  if (workspacePluginDefaults && !params.link) {
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

  // Enqueue deferred (non-markdown) files for background download.
  const deferredFiles = result?.deferred_files;
  if (deferredFiles && deferredFiles.length > 0 && params.remoteId) {
    try {
      const { getServerUrl, getToken } = await import("$lib/auth");
      const { initDeferredQueue, enqueueDeferredFiles } = await import("./deferredFileQueue");
      const deferredServerUrl = getServerUrl();
      const deferredToken = getToken();
      if (deferredServerUrl && deferredToken) {
        initDeferredQueue(workspaceApi, deferredServerUrl, deferredToken);
        enqueueDeferredFiles(params.remoteId, deferredFiles);
      }
    } catch (e) {
      console.warn("[workspaceProviderService] Failed to enqueue deferred files:", e);
    }
  }

  return {
    localId: localWs.id,
    filesImported: Number(result?.files_imported ?? 0),
    filesResumedSkip: Number(result?.files_resumed_skip ?? 0),
  };
}
