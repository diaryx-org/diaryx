<script lang="ts">
  /**
   * WorkspaceManagement - Manage all workspaces from settings.
   *
   * Features:
   * - When logged in: two sections (Synced with limit counter, Local)
   * - When logged out: single section (all workspaces are local)
   * - Toggle sync status (stop syncing / start syncing)
   * - Delete from server as a separate action from local delete
   * - Rename workspaces
   */
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Progress } from "$lib/components/ui/progress";
  import { Separator } from "$lib/components/ui/separator";
  import {
    Loader2,
    Pencil,
    Trash2,
    Check,
    X,
    HardDrive,
    Cloud,
    CloudOff,
    CloudUpload,
  } from "@lucide/svelte";
  import {
    getAuthState,
    getWorkspaces,
    getWorkspaceLimit,
    renameServerWorkspace,
    deleteServerWorkspace,
    createServerWorkspace,
    refreshUserInfo,
    uploadWorkspaceSnapshot,
    getServerUrl,
    isSyncEnabled,
    enableSync,
    setActiveWorkspaceId,
  } from "$lib/auth";
  import { getBackend, createApi } from "$lib/backend";
  import {
    disconnectWorkspace,
    setWorkspaceId,
    setWorkspaceServer,
  } from "$lib/crdt/workspaceCrdtBridge";
  import {
    isWorkspaceLocal,
    removeLocalWorkspace,
    renameLocalWorkspace,
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    setWorkspaceIsLocal,
    promoteLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { deleteLocalWorkspaceData } from "$lib/settings/clearData";
  import {
    completeSyncActionStatus,
    getSyncActionStatus,
    resetSyncActionStatus,
    setSyncActionStatus,
  } from "./syncActionStatusStore.svelte";
  import {
    buildWorkspaceSnapshotUploadBlob,
    findWorkspaceRootPath,
  } from "./workspaceSnapshotUpload";
  import { toast } from "svelte-sonner";

  let authState = $derived(getAuthState());
  let serverWorkspaces = $derived(getWorkspaces());
  let workspaceLimit = $derived(getWorkspaceLimit());
  let currentId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());
  let allLocal = $derived(getLocalWorkspaces());

  // Synced workspaces: on server AND not flagged as local-only in registry.
  // Deduplicates by ID defensively in case the server returns duplicate entries.
  let syncedWorkspaces = $derived.by(() => {
    if (!authState.isAuthenticated) return [];
    const seen = new Set<string>();
    return serverWorkspaces.filter(sw => {
      if (seen.has(sw.id)) return false;
      seen.add(sw.id);
      const localEntry = allLocal.find(lw => lw.id === sw.id);
      return !localEntry || !localEntry.isLocal;
    });
  });

  // Local workspaces: everything in registry not in the synced list.
  // Deduplicates by ID defensively.
  let localWorkspaces = $derived.by(() => {
    const syncedIds = new Set(syncedWorkspaces.map(w => w.id));
    const seen = new Set<string>();
    return allLocal.filter(w => {
      if (syncedIds.has(w.id) || seen.has(w.id)) return false;
      seen.add(w.id);
      return true;
    });
  });

  let hasAnyWorkspaces = $derived(syncedWorkspaces.length > 0 || localWorkspaces.length > 0);
  let canCreateServer = $derived(authState.isAuthenticated && serverWorkspaces.length < workspaceLimit);
  let syncActionStatus = $derived(getSyncActionStatus());

  // Rename state
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let renameLoading = $state(false);

  // Action state (delete, toggle sync)
  let actionId = $state<string | null>(null);
  let actionLoading = $state(false);
  let confirmAction = $state<{ id: string; type: 'delete-local' | 'delete-server' } | null>(null);

  function startRename(id: string, currentName: string) {
    renamingId = id;
    renameValue = currentName;
  }

  function cancelRename() {
    renamingId = null;
    renameValue = "";
  }

  async function submitRename() {
    if (!renamingId || !renameValue.trim()) return;
    renameLoading = true;
    try {
      const isServer = serverWorkspaces.some(w => w.id === renamingId);
      if (isServer) {
        await renameServerWorkspace(renamingId!, renameValue.trim());
      }
      renameLocalWorkspace(renamingId!, renameValue.trim());
      toast.success("Workspace renamed");
      cancelRename();
    } catch (e: any) {
      toast.error(e?.message || "Failed to rename workspace");
    } finally {
      renameLoading = false;
    }
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") submitRename();
    else if (e.key === "Escape") cancelRename();
  }

  function formatBytes(bytes: number): string {
    if (bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
    const value = bytes / Math.pow(1024, index);
    return `${value.toFixed(value < 10 && index > 0 ? 1 : 0)} ${units[index]}`;
  }

  function localWorkspaceHasServerCopy(id: string): boolean {
    return authState.isAuthenticated && serverWorkspaces.some(w => w.id === id);
  }

  /** Stop syncing a workspace — marks it as local, server copy untouched. */
  async function handleStopSync(id: string) {
    setWorkspaceIsLocal(id, true);

    // If this workspace is currently open, tear down active sync immediately.
    if (id === currentId) {
      disconnectWorkspace();
      await setWorkspaceId(null);
    }

    toast.success(
      id === currentId
        ? "Workspace set to local-only and disconnected from sync"
        : "Workspace set to local-only",
    );
  }

  /** Start syncing a local workspace by linking/creating on server and seeding via snapshot. */
  async function handleStartSync(id: string, name: string) {
    const normalizedName = name.trim();
    if (!normalizedName) {
      toast.error("Workspace must have a name before enabling sync");
      return;
    }
    const workspaceNameForServer = normalizedName;
    const displayName = normalizedName;
    const isCurrentWorkspace = currentId === id;

    let createdWorkspaceId: string | null = null;
    actionLoading = true;
    actionId = id;

    setSyncActionStatus({
      active: true,
      workspaceId: id,
      workspaceName: displayName,
      progress: 8,
      tone: "info",
      message: `Starting sync for "${displayName}"...`,
    });

    try {
      let serverWorkspaceId: string;
      let serverWorkspaceSource: "existing-id" | "existing-name" | "created" = "existing-id";

      const existingById = serverWorkspaces.find(w => w.id === id);
      if (existingById) {
        serverWorkspaceId = existingById.id;
      } else {
        setSyncActionStatus({
          progress: 20,
          message: "Checking for an existing cloud workspace...",
        });
        const existingByName = serverWorkspaces.find(w => w.name.trim() === workspaceNameForServer);
        if (existingByName) {
          serverWorkspaceId = existingByName.id;
          serverWorkspaceSource = "existing-name";
        } else {
          if (!canCreateServer) {
            throw new Error("Cannot start sync: synced workspace limit reached.");
          }

          setSyncActionStatus({
            progress: 44,
            message: "Creating cloud workspace...",
          });

          try {
            const created = await createServerWorkspace(workspaceNameForServer);
            serverWorkspaceId = created.id;
            serverWorkspaceSource = "created";
            createdWorkspaceId = created.id;
          } catch (e: any) {
            if (e?.statusCode !== 409) {
              throw e;
            }

            // Workspace list may be stale (e.g., created from another device); refresh and relink.
            setSyncActionStatus({
              progress: 60,
              message: "Workspace exists on cloud. Refreshing server list...",
            });
            await refreshUserInfo();
            const refreshed = getWorkspaces();
            const match = refreshed.find(w => w.name.trim() === workspaceNameForServer);
            if (!match) {
              throw new Error("A workspace with that name already exists on the server.");
            }
            serverWorkspaceId = match.id;
            serverWorkspaceSource = "existing-name";
          }
        }
      }

      // New local->cloud migration path: seed a new cloud workspace via snapshot
      // before enabling sync to avoid a heavy regular CRDT bootstrap in the browser.
      if (isCurrentWorkspace && serverWorkspaceSource === "created") {
        setSyncActionStatus({
          progress: 52,
          message: "Preparing workspace snapshot...",
        });

        const backend = await getBackend();
        const api = createApi(backend);
        const workspaceRootPath = await findWorkspaceRootPath(api, backend);

        if (workspaceRootPath) {
          const snapshot = await buildWorkspaceSnapshotUploadBlob(
            api,
            workspaceRootPath,
            (progress) => {
              if (progress.phase === "scan") {
                setSyncActionStatus({
                  progress: 56,
                  message: progress.detail ?? "Scanning workspace files...",
                });
                return;
              }

              const ratio = progress.totalFiles > 0
                ? progress.completedFiles / progress.totalFiles
                : 0;
              setSyncActionStatus({
                progress: 56 + Math.round(ratio * 18),
                message: progress.totalFiles > 0
                  ? `Preparing snapshot (${progress.completedFiles}/${progress.totalFiles})...`
                  : "Preparing snapshot...",
              });
            },
          );

          if (snapshot.filesPlanned > 0 && snapshot.blob.size > 0) {
            setSyncActionStatus({
              progress: 74,
              message: "Uploading snapshot to cloud...",
            });

            const uploadResult = await uploadWorkspaceSnapshot(
              serverWorkspaceId,
              snapshot.blob,
              "replace",
              true,
              (uploadedBytes, totalBytes) => {
                const ratio = totalBytes > 0 ? uploadedBytes / totalBytes : 0;
                setSyncActionStatus({
                  progress: 74 + Math.round(ratio * 20),
                  message: totalBytes > 0
                    ? `Uploading snapshot (${formatBytes(uploadedBytes)} / ${formatBytes(totalBytes)})...`
                    : "Uploading snapshot...",
                });
              },
            );

            if (!uploadResult) {
              throw new Error("Snapshot upload failed");
            }

            if (snapshot.attachmentReadFailures > 0) {
              toast.warning("Some attachments were skipped", {
                description: `${snapshot.attachmentReadFailures} attachment file(s) could not be included.`,
              });
            }

            setSyncActionStatus({
              progress: 94,
              message: `Snapshot uploaded (${uploadResult.files_imported} files).`,
            });
          } else {
            setSyncActionStatus({
              progress: 90,
              message: "Workspace is empty. Skipping snapshot upload.",
            });
          }
        } else {
          setSyncActionStatus({
            progress: 90,
            message: "No root index found. Skipping snapshot upload.",
          });
        }
      }

      setSyncActionStatus({
        progress: 96,
        message: "Finalizing sync setup...",
      });

      if (id === serverWorkspaceId) {
        setWorkspaceIsLocal(id, false);
      } else {
        promoteLocalWorkspace(id, serverWorkspaceId);
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

      createdWorkspaceId = null;
      const linkedMessage = serverWorkspaceSource === "existing-name"
        ? `Linked "${displayName}" to existing cloud workspace.`
        : `Workspace "${displayName}" is now synced.`;
      completeSyncActionStatus("success", linkedMessage);
      toast.success(
        serverWorkspaceSource === "existing-name"
          ? "Linked to existing cloud workspace"
          : "Workspace is now synced",
      );
    } catch (e: any) {
      if (createdWorkspaceId) {
        try {
          await deleteServerWorkspace(createdWorkspaceId);
          await refreshUserInfo();
        } catch (cleanupError) {
          console.warn(
            `[WorkspaceManagement] Failed to clean up workspace ${createdWorkspaceId} after sync start error:`,
            cleanupError,
          );
        }
      }
      const message = e?.message || "Failed to sync workspace";
      completeSyncActionStatus("error", message);
      toast.error(message);
    } finally {
      actionLoading = false;
      actionId = null;
    }
  }

  /** Delete a workspace from the server only — keeps local data, marks as local. */
  async function handleDeleteFromServer(id: string) {
    if (confirmAction?.id !== id || confirmAction?.type !== 'delete-server') {
      confirmAction = { id, type: 'delete-server' };
      return;
    }

    actionLoading = true;
    actionId = id;
    try {
      await deleteServerWorkspace(id);
      // Keep local data but mark as local-only
      setWorkspaceIsLocal(id, true);
      toast.success("Deleted from server");
      confirmAction = null;
    } catch (e: any) {
      toast.error(e?.message || "Failed to delete from server");
    } finally {
      actionLoading = false;
      actionId = null;
    }
  }

  /** Delete a workspace's local data. */
  async function handleDeleteLocal(id: string) {
    if (confirmAction?.id !== id || confirmAction?.type !== 'delete-local') {
      confirmAction = { id, type: 'delete-local' };
      return;
    }

    actionLoading = true;
    actionId = id;
    try {
      const ws = allLocal.find(w => w.id === id);
      await deleteLocalWorkspaceData(id, ws?.name);
      removeLocalWorkspace(id);
      toast.success("Workspace deleted");
      confirmAction = null;
    } catch (e: any) {
      toast.error(e?.message || "Failed to delete workspace");
    } finally {
      actionLoading = false;
      actionId = null;
    }
  }

  function cancelConfirm() {
    confirmAction = null;
  }
</script>

{#snippet renameRow(_wsId: string)}
  <Input
    bind:value={renameValue}
    onkeydown={handleRenameKeydown}
    class="h-7 text-sm flex-1"
    disabled={renameLoading}
  />
  <Button
    variant="ghost"
    size="icon"
    class="size-6"
    onclick={submitRename}
    disabled={renameLoading || !renameValue.trim()}
  >
    {#if renameLoading}
      <Loader2 class="size-3.5 animate-spin" />
    {:else}
      <Check class="size-3.5" />
    {/if}
  </Button>
  <Button
    variant="ghost"
    size="icon"
    class="size-6"
    onclick={cancelRename}
    disabled={renameLoading}
  >
    <X class="size-3.5" />
  </Button>
{/snippet}

{#snippet confirmRow(id: string, label: string, onConfirm: () => void)}
  <Button
    variant="destructive"
    size="sm"
    class="h-6 text-xs"
    onclick={onConfirm}
    disabled={actionLoading}
  >
    {#if actionLoading && actionId === id}
      <Loader2 class="size-3 animate-spin mr-1" />
    {/if}
    {label}
  </Button>
  <Button
    variant="ghost"
    size="icon"
    class="size-6"
    onclick={cancelConfirm}
  >
    <X class="size-3" />
  </Button>
{/snippet}

{#if hasAnyWorkspaces}
  <div class="space-y-4">
    {#if syncActionStatus.message}
      <div
        class="rounded-md border p-2 space-y-2 {syncActionStatus.tone === 'error'
          ? 'border-destructive/40 bg-destructive/5'
          : syncActionStatus.tone === 'success'
            ? 'border-green-500/40 bg-green-500/5'
            : 'bg-muted/50'}"
      >
        <div class="flex items-start justify-between gap-2">
          <p
            class="text-xs {syncActionStatus.tone === 'error'
              ? 'text-destructive'
              : syncActionStatus.tone === 'success'
                ? 'text-green-700 dark:text-green-300'
                : 'text-muted-foreground'}"
          >
            {syncActionStatus.message}
          </p>
          {#if !syncActionStatus.active}
            <Button
              variant="ghost"
              size="icon"
              class="size-5 shrink-0"
              onclick={resetSyncActionStatus}
              aria-label="Dismiss sync status"
            >
              <X class="size-3" />
            </Button>
          {/if}
        </div>
        {#if syncActionStatus.active}
          <Progress value={syncActionStatus.progress} class="h-1.5" />
        {/if}
      </div>
    {/if}

    <!-- Synced workspaces section (only when logged in) -->
    {#if authState.isAuthenticated && syncedWorkspaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <Cloud class="size-3.5 text-muted-foreground" />
            Synced Workspaces
          </h3>
          <span class="text-xs text-muted-foreground">
            {syncedWorkspaces.length} / {workspaceLimit}
          </span>
        </div>

        <Separator />

        <div class="space-y-1">
          {#each syncedWorkspaces as ws (ws.id)}
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-muted/50">
              {#if renamingId === ws.id}
                {@render renameRow(ws.id)}
              {:else}
                <span class="flex items-center gap-1.5 flex-1 min-w-0">
                  {#if isWorkspaceLocal(ws.id)}
                    <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
                  {:else}
                    <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="text-sm truncate">{ws.name}</span>
                  {#if ws.id === currentId}
                    <span class="text-[10px] px-1 py-0.5 rounded bg-primary/10 text-primary shrink-0">active</span>
                  {/if}
                </span>
                <div class="flex items-center gap-0.5">
                  {#if confirmAction?.id === ws.id}
                    {#if confirmAction.type === 'delete-server'}
                      {@render confirmRow(ws.id, "Delete from cloud", () => handleDeleteFromServer(ws.id))}
                    {:else}
                      {@render confirmRow(ws.id, "Delete locally", () => handleDeleteLocal(ws.id))}
                    {/if}
                  {:else}
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => startRename(ws.id, ws.name)}
                      title="Rename"
                    >
                      <Pencil class="size-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => handleStopSync(ws.id)}
                      title="Stop syncing"
                    >
                      <CloudOff class="size-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => handleDeleteFromServer(ws.id)}
                      disabled={ws.id === currentId}
                      title={ws.id === currentId ? "Switch to another workspace first" : "Delete from cloud"}
                    >
                      <Trash2 class="size-3" />
                    </Button>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Local workspaces section -->
    {#if localWorkspaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <HardDrive class="size-3.5 text-muted-foreground" />
            {#if authState.isAuthenticated}
              Local Workspaces
            {:else}
              Workspaces
            {/if}
          </h3>
          <span class="text-xs text-muted-foreground">
            {localWorkspaces.length}
          </span>
        </div>

        {#if syncedWorkspaces.length > 0}
          <Separator />
        {/if}

        <div class="space-y-1">
          {#each localWorkspaces as ws (ws.id)}
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-muted/50">
              {#if renamingId === ws.id}
                {@render renameRow(ws.id)}
              {:else}
                <span class="flex items-center gap-1.5 flex-1 min-w-0">
                  <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
                  <span class="text-sm truncate">{ws.name}</span>
                  {#if localWorkspaceHasServerCopy(ws.id)}
                    <span
                      class="text-[10px] px-1 py-0.5 rounded bg-sky-500/10 text-sky-700 dark:text-sky-300 shrink-0"
                    >
                      cloud copy
                    </span>
                  {/if}
                  {#if ws.id === currentId}
                    <span class="text-[10px] px-1 py-0.5 rounded bg-primary/10 text-primary shrink-0">active</span>
                  {/if}
                </span>
                <div class="flex items-center gap-0.5">
                  {#if confirmAction?.id === ws.id}
                    {#if confirmAction.type === 'delete-server'}
                      {@render confirmRow(ws.id, "Delete cloud copy", () => handleDeleteFromServer(ws.id))}
                    {:else}
                      {@render confirmRow(ws.id, "Delete locally", () => handleDeleteLocal(ws.id))}
                    {/if}
                  {:else}
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => startRename(ws.id, ws.name)}
                      title="Rename"
                    >
                      <Pencil class="size-3" />
                    </Button>
                    {#if authState.isAuthenticated}
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        onclick={() => handleStartSync(ws.id, ws.name)}
                        disabled={!canCreateServer || (actionLoading && actionId === ws.id)}
                        title={canCreateServer ? "Start syncing" : "Synced workspace limit reached"}
                      >
                        {#if actionLoading && actionId === ws.id}
                          <Loader2 class="size-3 animate-spin" />
                        {:else}
                          <CloudUpload class="size-3" />
                        {/if}
                      </Button>
                      {#if localWorkspaceHasServerCopy(ws.id)}
                        <Button
                          variant="ghost"
                          size="icon"
                          class="size-6"
                          onclick={() => handleDeleteFromServer(ws.id)}
                          title="Delete cloud copy"
                        >
                          <CloudOff class="size-3" />
                        </Button>
                      {/if}
                    {/if}
                    <Button
                      variant="ghost"
                      size="icon"
                      class="size-6"
                      onclick={() => handleDeleteLocal(ws.id)}
                      disabled={ws.id === currentId}
                      title={ws.id === currentId ? "Switch to another workspace first" : "Delete"}
                    >
                      <Trash2 class="size-3" />
                    </Button>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>

        {#if authState.isAuthenticated}
          <p class="text-xs text-muted-foreground">
            Local workspaces are stored on this device only.
          </p>
        {/if}
      </div>
    {/if}

  </div>
{/if}
