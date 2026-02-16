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
  } from "$lib/auth";
  import {
    isWorkspaceLocal,
    removeLocalWorkspace,
    renameLocalWorkspace,
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    setWorkspaceIsLocal,
    promoteLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry";
  import { deleteLocalWorkspaceData } from "$lib/settings/clearData";
  import { toast } from "svelte-sonner";

  let authState = $derived(getAuthState());
  let serverWorkspaces = $derived(getWorkspaces());
  let workspaceLimit = $derived(getWorkspaceLimit());
  let currentId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());
  let allLocal = $derived(getLocalWorkspaces());

  // Synced workspaces: on server AND not flagged as local-only in registry
  let syncedWorkspaces = $derived.by(() => {
    if (!authState.isAuthenticated) return [];
    return serverWorkspaces.filter(sw => {
      const localEntry = allLocal.find(lw => lw.id === sw.id);
      return !localEntry || !localEntry.isLocal;
    });
  });

  // Local workspaces: everything in registry not in the synced list
  let localWorkspaces = $derived.by(() => {
    const syncedIds = new Set(syncedWorkspaces.map(w => w.id));
    return allLocal.filter(w => !syncedIds.has(w.id));
  });

  let hasAnyWorkspaces = $derived(syncedWorkspaces.length > 0 || localWorkspaces.length > 0);
  let canCreateServer = $derived(authState.isAuthenticated && serverWorkspaces.length < workspaceLimit);

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

  /** Stop syncing a workspace — marks it as local, server copy untouched. */
  function handleStopSync(id: string) {
    setWorkspaceIsLocal(id, true);
    toast.success("Workspace set to local-only");
  }

  /** Start syncing a local workspace — creates on server, promotes in registry. */
  async function handleStartSync(id: string, name: string) {
    if (!canCreateServer) {
      toast.error("Synced workspace limit reached");
      return;
    }
    actionLoading = true;
    actionId = id;
    try {
      const ws = await createServerWorkspace(name);
      promoteLocalWorkspace(id, ws.id);
      toast.success("Workspace is now synced");
    } catch (e: any) {
      if (e?.statusCode === 409) {
        toast.error("A workspace with that name already exists on the server");
      } else {
        toast.error(e?.message || "Failed to sync workspace");
      }
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
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-muted/50 group">
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
                <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
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
                    {#if ws.id !== currentId}
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        onclick={() => handleDeleteFromServer(ws.id)}
                        title="Delete from cloud"
                      >
                        <Trash2 class="size-3" />
                      </Button>
                    {/if}
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
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-muted/50 group">
              {#if renamingId === ws.id}
                {@render renameRow(ws.id)}
              {:else}
                <span class="flex items-center gap-1.5 flex-1 min-w-0">
                  <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
                  <span class="text-sm truncate">{ws.name}</span>
                  {#if ws.id === currentId}
                    <span class="text-[10px] px-1 py-0.5 rounded bg-primary/10 text-primary shrink-0">active</span>
                  {/if}
                </span>
                <div class="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                  {#if confirmAction?.id === ws.id}
                    {@render confirmRow(ws.id, "Confirm delete", () => handleDeleteLocal(ws.id))}
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
                    {#if canCreateServer}
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        onclick={() => handleStartSync(ws.id, ws.name)}
                        disabled={actionLoading && actionId === ws.id}
                        title="Start syncing"
                      >
                        {#if actionLoading && actionId === ws.id}
                          <Loader2 class="size-3 animate-spin" />
                        {:else}
                          <CloudUpload class="size-3" />
                        {/if}
                      </Button>
                    {/if}
                    {#if ws.id !== currentId}
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        onclick={() => handleDeleteLocal(ws.id)}
                        title="Delete"
                      >
                        <Trash2 class="size-3" />
                      </Button>
                    {/if}
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

    {#if currentId}
      <p class="text-xs text-muted-foreground">
        The active workspace cannot be deleted. Switch to a different workspace first.
      </p>
    {/if}
  </div>
{/if}
