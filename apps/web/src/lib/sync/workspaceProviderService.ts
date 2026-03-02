/**
 * Workspace Provider Service
 *
 * Centralizes provider-agnostic workspace operations (link, unlink, download).
 * Components call these functions instead of directly orchestrating auth APIs,
 * CRDT bridge, and workspace registry.
 *
 * Provider readiness and remote workspace listing use the existing auth store
 * (host-side), while link/unlink/download orchestrate the full lifecycle.
 */

import {
  isAuthenticated,
  getAuthState,
  getWorkspaces,
  getWorkspaceLimit,
  getServerUrl,
  createServerWorkspace,
  deleteServerWorkspace,
  uploadWorkspaceSnapshot,
  downloadWorkspaceSnapshot,
  refreshUserInfo,
  enableSync,
  isSyncEnabled,
  setActiveWorkspaceId,
} from "$lib/auth";
import { getBackend, createApi } from "$lib/backend";
import {
  setWorkspaceServer,
  setWorkspaceId,
  disconnectWorkspace,
} from "$lib/crdt/workspaceCrdtBridge";
import {
  getCurrentWorkspaceId,
  addLocalWorkspace,
  setCurrentWorkspaceId,
  setWorkspaceIsLocal,
  promoteLocalWorkspace,
  createLocalWorkspace,
  getWorkspaceStorageType,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import {
  buildWorkspaceSnapshotUploadBlob,
  findWorkspaceRootPath,
} from "$lib/settings/workspaceSnapshotUpload";

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

// ============================================================================
// Query functions
// ============================================================================

/**
 * Check whether a workspace provider is ready (authenticated, has quota, etc.).
 * Currently only the built-in Diaryx Sync provider is supported.
 */
export function getProviderStatus(_pluginId: string): ProviderStatus {
  if (!isAuthenticated()) {
    return { ready: false, message: "Sign in to enable sync" };
  }
  const auth = getAuthState();
  if (auth.tier !== "plus") {
    return { ready: false, message: "Upgrade to Plus for sync" };
  }
  const limit = getWorkspaceLimit();
  const current = getWorkspaces().length;
  if (current >= limit) {
    return { ready: false, message: `Workspace limit reached (${current}/${limit})` };
  }
  return { ready: true };
}

/**
 * List remote workspaces from a provider that are NOT already linked locally.
 */
export function listRemoteWorkspaces(_pluginId: string): RemoteWorkspace[] {
  if (!isAuthenticated()) return [];
  return getWorkspaces().map((w) => ({ id: w.id, name: w.name }));
}

/**
 * List remote workspaces that have no local counterpart.
 */
export function listUnlinkedRemoteWorkspaces(
  _pluginId: string,
  localWorkspaceServerIds: Set<string>,
): RemoteWorkspace[] {
  return listRemoteWorkspaces(_pluginId).filter(
    (w) => !localWorkspaceServerIds.has(w.id),
  );
}

// ============================================================================
// Mutation functions
// ============================================================================

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / Math.pow(1024, index);
  return `${value.toFixed(value < 10 && index > 0 ? 1 : 0)} ${units[index]}`;
}

/**
 * Link a local workspace to a remote provider.
 *
 * Encapsulates: check/create server workspace, build+upload snapshot,
 * initialize CRDT, connect WebSocket, wait for initial sync, promote
 * local workspace, enable sync.
 */
export async function linkWorkspace(
  _pluginId: string,
  params: { localId: string; name: string; remoteId?: string },
  onProgress?: ProgressCallback,
): Promise<{ remoteId: string }> {
  const { localId, name } = params;
  const isCurrentWorkspace = getCurrentWorkspaceId() === localId;
  let createdWorkspaceId: string | null = null;

  try {
    // Step 1: Find or create server workspace
    onProgress?.({ percent: 8, message: `Starting sync for "${name}"...` });

    let serverWorkspaceId: string;
    let serverWorkspaceSource: "existing-id" | "existing-name" | "created" =
      "existing-id";

    if (params.remoteId) {
      serverWorkspaceId = params.remoteId;
    } else {
      const serverWorkspaces = getWorkspaces();
      const existingById = serverWorkspaces.find((w) => w.id === localId);
      if (existingById) {
        serverWorkspaceId = existingById.id;
      } else {
        onProgress?.({
          percent: 20,
          message: "Checking for an existing cloud workspace...",
        });
        const existingByName = serverWorkspaces.find(
          (w) => w.name.trim() === name.trim(),
        );
        if (existingByName) {
          serverWorkspaceId = existingByName.id;
          serverWorkspaceSource = "existing-name";
        } else {
          const status = getProviderStatus(_pluginId);
          if (!status.ready) {
            throw new Error(
              status.message || "Cannot start sync: provider not ready",
            );
          }

          onProgress?.({ percent: 44, message: "Creating cloud workspace..." });

          try {
            const created = await createServerWorkspace(name.trim());
            serverWorkspaceId = created.id;
            serverWorkspaceSource = "created";
            createdWorkspaceId = created.id;
          } catch (e: any) {
            if (e?.statusCode !== 409) throw e;

            onProgress?.({
              percent: 60,
              message:
                "Workspace exists on cloud. Refreshing server list...",
            });
            await refreshUserInfo();
            const refreshed = getWorkspaces();
            const match = refreshed.find(
              (w) => w.name.trim() === name.trim(),
            );
            if (!match) {
              throw new Error(
                "A workspace with that name already exists on the server.",
              );
            }
            serverWorkspaceId = match.id;
            serverWorkspaceSource = "existing-name";
          }
        }
      }
    }

    // Step 2: Upload snapshot for new server workspaces
    if (isCurrentWorkspace && serverWorkspaceSource === "created") {
      onProgress?.({ percent: 52, message: "Preparing workspace snapshot..." });

      const backend = await getBackend();
      const api = createApi(backend);
      const workspaceRootPath = await findWorkspaceRootPath(api, backend);

      if (workspaceRootPath) {
        const snapshot = await buildWorkspaceSnapshotUploadBlob(
          api,
          workspaceRootPath,
          (progress) => {
            if (progress.phase === "scan") {
              onProgress?.({
                percent: 56,
                message: progress.detail ?? "Scanning workspace files...",
              });
              return;
            }
            const ratio =
              progress.totalFiles > 0
                ? progress.completedFiles / progress.totalFiles
                : 0;
            onProgress?.({
              percent: 56 + Math.round(ratio * 18),
              message:
                progress.totalFiles > 0
                  ? `Preparing snapshot (${progress.completedFiles}/${progress.totalFiles})...`
                  : "Preparing snapshot...",
            });
          },
        );

        if (snapshot.filesPlanned > 0 && snapshot.blob.size > 0) {
          onProgress?.({
            percent: 74,
            message: "Uploading snapshot to cloud...",
          });

          const uploadResult = await uploadWorkspaceSnapshot(
            serverWorkspaceId,
            snapshot.blob,
            "replace",
            true,
            (uploadedBytes, totalBytes) => {
              const ratio =
                totalBytes > 0 ? uploadedBytes / totalBytes : 0;
              onProgress?.({
                percent: 74 + Math.round(ratio * 20),
                message:
                  totalBytes > 0
                    ? `Uploading snapshot (${formatBytes(uploadedBytes)} / ${formatBytes(totalBytes)})...`
                    : "Uploading snapshot...",
              });
            },
          );

          if (!uploadResult) {
            throw new Error("Snapshot upload failed");
          }

          onProgress?.({
            percent: 94,
            message: `Snapshot uploaded (${uploadResult.files_imported} files).`,
          });
        } else {
          onProgress?.({
            percent: 90,
            message: "Workspace is empty. Skipping snapshot upload.",
          });
        }
      } else {
        onProgress?.({
          percent: 90,
          message: "No root index found. Skipping snapshot upload.",
        });
      }
    }

    // Step 3: Finalize sync setup
    onProgress?.({ percent: 96, message: "Finalizing sync setup..." });

    if (localId === serverWorkspaceId) {
      setWorkspaceIsLocal(localId, false);
    } else {
      promoteLocalWorkspace(localId, serverWorkspaceId);
    }

    if (isCurrentWorkspace) {
      setActiveWorkspaceId(serverWorkspaceId);
      await setWorkspaceId(serverWorkspaceId);
      const syncServerUrl = getServerUrl();
      if (syncServerUrl) {
        if (!isSyncEnabled()) {
          enableSync();
        }
        await setWorkspaceServer(syncServerUrl);
      }
    }

    createdWorkspaceId = null; // Success — don't clean up
    onProgress?.({ percent: 100, message: "Sync enabled." });

    return { remoteId: serverWorkspaceId };
  } catch (e) {
    // Clean up partially-created server workspace on failure
    if (createdWorkspaceId) {
      try {
        await deleteServerWorkspace(createdWorkspaceId);
        await refreshUserInfo();
      } catch (cleanupError) {
        console.warn(
          `[workspaceProviderService] Failed to clean up workspace ${createdWorkspaceId}:`,
          cleanupError,
        );
      }
    }
    throw e;
  }
}

/**
 * Unlink a workspace from a remote provider.
 * Marks it local-only and disconnects active sync if needed.
 */
export async function unlinkWorkspace(
  _pluginId: string,
  localId: string,
): Promise<void> {
  setWorkspaceIsLocal(localId, true);

  if (localId === getCurrentWorkspaceId()) {
    disconnectWorkspace();
    await setWorkspaceId(null);
  }
}

/**
 * Download a remote workspace and create a local copy.
 * Optionally links it to the provider for ongoing sync.
 */
export async function downloadWorkspace(
  _pluginId: string,
  params: { remoteId: string; name: string; link?: boolean },
  onProgress?: ProgressCallback,
): Promise<{ localId: string }> {
  const { remoteId, name } = params;

  onProgress?.({ percent: 10, message: "Creating local workspace..." });

  const localWs = createLocalWorkspace(name);

  // Register locally
  addLocalWorkspace({ id: localWs.id, name });
  setCurrentWorkspaceId(localWs.id);

  // Create backend for this workspace and import the snapshot
  const storageType = getWorkspaceStorageType(localWs.id);
  const backend = await getBackend(localWs.id, name, storageType);
  const api = createApi(backend);

  const workspaceDir = backend
    .getWorkspacePath()
    .replace(/\/index\.md$/, "")
    .replace(/\/README\.md$/, "");

  // Update registry with the resolved filesystem path (Tauri)
  if (workspaceDir && workspaceDir !== ".") {
    addLocalWorkspace({ id: localWs.id, name, path: workspaceDir });
  }

  try {
    await api.createWorkspace(workspaceDir, name);
  } catch {
    // May already exist
  }

  // Download and import snapshot
  onProgress?.({ percent: 30, message: "Downloading workspace..." });

  const blob = await downloadWorkspaceSnapshot(remoteId, true);
  if (blob && blob.size > 100) {
    onProgress?.({ percent: 60, message: "Importing files..." });
    const file = new File([blob], "snapshot.zip", {
      type: "application/zip",
    });
    await backend.importFromZip(file, workspaceDir);
  }

  onProgress?.({ percent: 90, message: "Finalizing..." });

  // Optionally link for ongoing sync
  if (params.link) {
    promoteLocalWorkspace(localWs.id, remoteId);
    setActiveWorkspaceId(remoteId);
    if (!isSyncEnabled()) {
      enableSync();
    }
  }

  onProgress?.({ percent: 100, message: "Done." });

  return { localId: localWs.id };
}
