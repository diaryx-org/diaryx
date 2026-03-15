<script lang="ts">
  /**
   * WorkspaceManagement - Manage all workspaces from settings.
   *
   * Features:
   * - Two sections: Synced workspaces and Local workspaces
   * - Link/unlink provider (replaces hardcoded start/stop sync)
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
    CloudDownload,
  } from "@lucide/svelte";
  import {
    getAuthState,
    getWorkspaces,
    renameServerWorkspace,
    deleteServerWorkspace,
  } from "$lib/auth";
  import { isTierLimitError } from "$lib/billing";
  import {
    removeLocalWorkspace,
    renameLocalWorkspace,
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    getServerWorkspaceId,
    isWorkspaceSyncEnabled,
    setPluginMetadata,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { deleteLocalWorkspaceData } from "$lib/settings/clearData";
  import { toast } from "svelte-sonner";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getProviderStatus,
    linkWorkspace,
    unlinkWorkspace,
    listRemoteWorkspaces,
    downloadWorkspace,
    type RemoteWorkspace,
  } from "$lib/sync/workspaceProviderService";
  import { tick } from "svelte";

  const pluginStore = getPluginStore();

  let authState = $derived(getAuthState());
  let serverWorkspaces = $derived(getWorkspaces());
  let currentId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());
  let allLocal = $derived(getLocalWorkspaces());
  let workspaceProviders = $derived(pluginStore.workspaceProviders);

  // Synced workspaces: local workspaces that have a server ID linked.
  let syncedWorkspaces = $derived.by(() => {
    const seen = new Set<string>();
    return allLocal.filter(ws => {
      if (seen.has(ws.id)) return false;
      seen.add(ws.id);
      return !!getServerWorkspaceId(ws.id);
    });
  });

  // Local workspaces: everything in registry not in the synced list.
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
  type ActionTone = "info" | "success" | "error";
  let syncActionStatus = $state<{
    active: boolean;
    workspaceId: string | null;
    workspaceName: string | null;
    progress: number;
    tone: ActionTone;
    message: string | null;
  }>({
    active: false,
    workspaceId: null,
    workspaceName: null,
    progress: 0,
    tone: "info",
    message: null,
  });

  function setSyncActionStatus(patch: Partial<typeof syncActionStatus>): void {
    syncActionStatus = { ...syncActionStatus, ...patch };
  }

  function completeSyncActionStatus(tone: ActionTone, message: string): void {
    syncActionStatus = {
      ...syncActionStatus,
      active: false,
      progress: 100,
      tone,
      message,
    };
  }

  function resetSyncActionStatus(): void {
    syncActionStatus = {
      active: false,
      workspaceId: null,
      workspaceName: null,
      progress: 0,
      tone: "info",
      message: null,
    };
  }

  // Cloud workspaces (remote-only, not linked locally)
  let cloudWorkspaces = $state<RemoteWorkspace[]>([]);
  let downloadingId = $state<string | null>(null);

  // Default provider for link button
  let defaultProvider = $derived(workspaceProviders[0] ?? null);
  let providerReady = $state(false);

  $effect(() => {
    const provider = defaultProvider;
    if (!provider) {
      providerReady = false;
      return;
    }
    void (async () => {
      const status = await getProviderStatus(provider.contribution.id);
      providerReady = status.ready;
    })();
  });

  // Load cloud workspaces when the provider is ready.
  $effect(() => {
    const provider = defaultProvider;
    if (!provider || !providerReady) {
      cloudWorkspaces = [];
      return;
    }
    void (async () => {
      try {
        const linkedServerIds = new Set(
          allLocal
            .map(ws => getServerWorkspaceId(ws.id))
            .filter((id): id is string => !!id),
        );
        const remote = await listRemoteWorkspaces(provider.contribution.id);
        cloudWorkspaces = remote.filter(w => !linkedServerIds.has(w.id));
      } catch (e) {
        console.warn("[WorkspaceManagement] Failed to list cloud workspaces:", e);
        cloudWorkspaces = [];
      }
    })();
  });

  // Rename state
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let renameLoading = $state(false);

  // Action state (delete, toggle sync)
  let actionId = $state<string | null>(null);
  let actionLoading = $state(false);
  let confirmAction = $state<{ id: string; type: 'delete-local' | 'delete-server' } | null>(null);
  let rootEl = $state<HTMLDivElement | null>(null);

  function getSettingsScrollContainer(): HTMLElement | null {
    const scrollContainer = rootEl?.closest("[data-settings-scroll-container]");
    return scrollContainer instanceof HTMLElement ? scrollContainer : null;
  }

  async function preserveSettingsScroll<T>(update: () => T | Promise<T>): Promise<T> {
    const scrollContainer = getSettingsScrollContainer();
    const savedScrollTop = scrollContainer?.scrollTop ?? null;

    try {
      return await update();
    } finally {
      if (savedScrollTop !== null) {
        await tick();
        const nextScrollContainer = getSettingsScrollContainer();
        if (nextScrollContainer) {
          const maxScrollTop = Math.max(
            0,
            nextScrollContainer.scrollHeight - nextScrollContainer.clientHeight,
          );
          nextScrollContainer.scrollTop = Math.min(savedScrollTop, maxScrollTop);
        }
      }
    }
  }

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

  /** Unlink a workspace from its provider — marks it as local, server copy untouched. */
  async function handleUnlink(id: string) {
    const providerId = defaultProvider?.contribution.id;
    if (providerId && isWorkspaceSyncEnabled(id)) {
      await unlinkWorkspace(providerId, id);
    } else {
      setPluginMetadata(id, "sync", null);
    }

    toast.success(
      id === currentId
        ? "Workspace set to local-only and disconnected from sync"
        : "Workspace set to local-only",
    );
  }

  /** Link a local workspace to a provider. */
  async function handleLink(id: string, name: string, remoteId?: string) {
    const providerId = defaultProvider?.contribution.id;
    if (!providerId) return;

    const normalizedName = name.trim();
    if (!normalizedName) {
      toast.error("Workspace must have a name before enabling sync");
      return;
    }

    actionLoading = true;
    actionId = id;

    setSyncActionStatus({
      active: true,
      workspaceId: id,
      workspaceName: normalizedName,
      progress: 8,
      tone: "info",
      message: `Starting sync for "${normalizedName}"...`,
    });

    try {
      await linkWorkspace(
        providerId,
        { localId: id, name: normalizedName, remoteId },
        (progress) => {
          setSyncActionStatus({
            progress: progress.percent,
            message: progress.message,
          });
        },
      );

      completeSyncActionStatus("success", `Workspace "${normalizedName}" is now synced.`);
      toast.success("Workspace is now synced");
    } catch (e: any) {
      const message = isTierLimitError(e)
        ? "Workspace sync limit reached. Free plans can sync one workspace; upgrade to sync more."
        : (e?.message || "Failed to sync workspace");
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
      await preserveSettingsScroll(() => {
        confirmAction = { id, type: 'delete-server' };
      });
      return;
    }

    const serverId = getServerWorkspaceId(id);
    if (!serverId) {
      confirmAction = null;
      toast.error("Workspace is not linked to a cloud copy");
      return;
    }

    await preserveSettingsScroll(async () => {
      actionLoading = true;
      actionId = id;
      try {
        await deleteServerWorkspace(serverId);
        await handleUnlink(id);
        toast.success("Deleted from server");
        confirmAction = null;
      } catch (e: any) {
        toast.error(e?.message || "Failed to delete from server");
      } finally {
        actionLoading = false;
        actionId = null;
      }
    });
  }

  /** Delete a workspace's local data. */
  async function handleDeleteLocal(id: string) {
    if (confirmAction?.id !== id || confirmAction?.type !== 'delete-local') {
      await preserveSettingsScroll(() => {
        confirmAction = { id, type: 'delete-local' };
      });
      return;
    }

    await preserveSettingsScroll(async () => {
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
    });
  }

  async function cancelConfirm() {
    await preserveSettingsScroll(() => {
      confirmAction = null;
    });
  }

  async function handleDownloadCloud(remote: RemoteWorkspace) {
    const providerId = defaultProvider?.contribution.id;
    if (!providerId) return;

    downloadingId = remote.id;
    setSyncActionStatus({
      active: true,
      workspaceId: null,
      workspaceName: remote.name,
      progress: 8,
      tone: "info",
      message: `Downloading "${remote.name}"...`,
    });

    try {
      await downloadWorkspace(
        providerId,
        { remoteId: remote.id, name: remote.name, link: true },
        (progress) => {
          setSyncActionStatus({
            progress: progress.percent,
            message: progress.message,
          });
        },
      );
      completeSyncActionStatus("success", `"${remote.name}" downloaded and linked.`);
      toast.success(`Workspace "${remote.name}" downloaded`);
    } catch (e: any) {
      completeSyncActionStatus("error", e?.message || "Failed to download workspace");
      toast.error(e?.message || "Failed to download workspace");
    } finally {
      downloadingId = null;
    }
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
  <div bind:this={rootEl} class="space-y-4">
    {#if syncActionStatus.message}
      <div
        class="rounded-md border p-2 space-y-2 {syncActionStatus.tone === 'error'
          ? 'border-destructive/40 bg-destructive/5'
          : syncActionStatus.tone === 'success'
            ? 'border-green-500/40 bg-green-500/5'
            : 'bg-secondary'}"
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

    <!-- Cloud-linked workspaces section -->
    {#if syncedWorkspaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <Cloud class="size-3.5 text-muted-foreground" />
            Cloud-linked Workspaces
          </h3>
          <span class="text-xs text-muted-foreground">
            {syncedWorkspaces.length}
          </span>
        </div>

        <Separator />

        <div class="space-y-1">
          {#each syncedWorkspaces as ws (ws.id)}
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-secondary">
              {#if renamingId === ws.id}
                {@render renameRow(ws.id)}
              {:else}
                <span class="flex items-center gap-1.5 flex-1 min-w-0">
                  <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                  <span class="text-sm truncate">{ws.name}</span>
                  <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">
                    {isWorkspaceSyncEnabled(ws.id) ? 'sync enabled' : 'publish only'}
                  </span>
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
                      onclick={() => isWorkspaceSyncEnabled(ws.id)
                        ? handleUnlink(ws.id)
                        : handleLink(ws.id, ws.name, getServerWorkspaceId(ws.id) ?? undefined)}
                      disabled={ws.id !== currentId || (actionLoading && actionId === ws.id)}
                      title={isWorkspaceSyncEnabled(ws.id)
                        ? 'Unlink from provider'
                        : (ws.id === currentId ? 'Enable sync using this cloud-linked workspace' : 'Switch to this workspace first to enable sync')}
                    >
                      {#if actionLoading && actionId === ws.id}
                        <Loader2 class="size-3 animate-spin" />
                      {:else if isWorkspaceSyncEnabled(ws.id)}
                        <CloudOff class="size-3" />
                      {:else}
                        <CloudUpload class="size-3" />
                      {/if}
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

    <!-- Cloud workspaces (remote-only, not linked locally) -->
    {#if providerReady && cloudWorkspaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <CloudDownload class="size-3.5 text-muted-foreground" />
            Cloud Workspaces
          </h3>
          <span class="text-xs text-muted-foreground">
            {cloudWorkspaces.length}
          </span>
        </div>

        <Separator />

        <div class="space-y-1">
          {#each cloudWorkspaces as remote (remote.id)}
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-secondary">
              <span class="flex items-center gap-1.5 flex-1 min-w-0">
                <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                <span class="text-sm truncate">{remote.name}</span>
                <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">cloud only</span>
              </span>
              <Button
                variant="ghost"
                size="icon"
                class="size-6"
                onclick={() => handleDownloadCloud(remote)}
                disabled={downloadingId === remote.id}
                title="Download to this device"
              >
                {#if downloadingId === remote.id}
                  <Loader2 class="size-3 animate-spin" />
                {:else}
                  <CloudDownload class="size-3" />
                {/if}
              </Button>
            </div>
          {/each}
        </div>

        <p class="text-xs text-muted-foreground">
          These workspaces exist on the server but are not on this device.
        </p>
      </div>
    {/if}

    <!-- Local workspaces section -->
    {#if localWorkspaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <HardDrive class="size-3.5 text-muted-foreground" />
            {#if syncedWorkspaces.length > 0}
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
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-secondary">
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
                <div class="flex items-center gap-0.5">
                  {#if confirmAction?.id === ws.id}
                    {#if confirmAction.type === 'delete-local'}
                      {@render confirmRow(ws.id, "Delete locally", () => handleDeleteLocal(ws.id))}
                    {:else}
                      {@render confirmRow(ws.id, "Delete cloud copy", () => handleDeleteFromServer(ws.id))}
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
                    {#if defaultProvider && providerReady}
                      <Button
                        variant="ghost"
                        size="icon"
                        class="size-6"
                        onclick={() => handleLink(ws.id, ws.name)}
                        disabled={ws.id !== currentId || (actionLoading && actionId === ws.id)}
                        title={ws.id === currentId
                          ? "Sync to cloud"
                          : "Switch to this workspace first to enable sync"}
                      >
                        {#if actionLoading && actionId === ws.id}
                          <Loader2 class="size-3 animate-spin" />
                        {:else}
                          <CloudUpload class="size-3" />
                        {/if}
                      </Button>
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

        {#if defaultProvider}
          <p class="text-xs text-muted-foreground">
            Free includes one synced workspace on up to two devices. Upgrade to Plus for more synced workspaces.
          </p>
        {/if}
      </div>
    {/if}

    {#if syncedWorkspaces.length > 0}
      <p class="text-xs text-muted-foreground">
        Cloud-linked workspaces can publish with auth only. Enable sync when you want faster server-side publishes.
      </p>
    {/if}

  </div>
{/if}
