<script lang="ts">
  import { tick } from "svelte";
  import * as Popover from "$lib/components/ui/popover";
  import { Input } from "$lib/components/ui/input";
  import {
    ChevronsUpDown,
    Check,
    Plus,
    Loader2,
    HardDrive,
    Cloud,
    Ellipsis,
    Pencil,
    Trash2,
  } from "@lucide/svelte";
  import {
    getServerWorkspaceId,
    getLocalWorkspaces,
    getWorkspaceStorageType,
    getCurrentWorkspaceId,
    renameLocalWorkspace,
    removeLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import type { StorageType } from "$lib/backend/storageType";
  import { switchWorkspace } from "$lib/workspace/switchWorkspace";
  import {
    getAuthState,
    getWorkspaces as getServerWorkspaces,
    renameServerWorkspace,
  } from "$lib/auth";
  import { toast } from "svelte-sonner";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getProviderStatus,
    listUnlinkedRemoteWorkspaces,
    downloadWorkspace,
    type RemoteWorkspace,
  } from "$lib/sync/workspaceProviderService";
  import { deleteLocalWorkspaceData } from "$lib/settings/clearData";
  import { BackendError } from "$lib/backend/interface";

  interface Props {
    onSwitchStart?: () => void;
    onSwitchComplete?: () => void;
    onAddWorkspace?: () => void;
    onWorkspaceMissing?: (ws: { id: string; name: string }) => void;
  }

  let { onSwitchStart, onSwitchComplete, onAddWorkspace, onWorkspaceMissing }: Props = $props();

  let open = $state(false);
  let switching = $state(false);

  const pluginStore = getPluginStore();

  // Derived state
  let allLocalWorkspaces = $derived(getLocalWorkspaces());
  let workspaceProviders = $derived(pluginStore.workspaceProviders);

  // Remote extras: unlinked remote workspaces per provider
  let remoteExtras = $state<Record<string, RemoteWorkspace[]>>({});

  // RemoteWorkspacePicker state
  let pickerProvider = $state<{ pluginId: string; label: string; workspaces: RemoteWorkspace[] } | null>(null);
  let downloading = $state<string | null>(null);

  // Local workspace list with sync indicator
  type LocalWorkspaceEntry = { id: string; name: string; synced: boolean };
  let workspaces = $derived.by(() => {
    const result: LocalWorkspaceEntry[] = [];
    for (const ws of allLocalWorkspaces) {
      const serverId = getServerWorkspaceId(ws.id);
      result.push({ id: ws.id, name: ws.name, synced: !!serverId });
    }
    return result;
  });

  // Show storage badges only when workspaces use mixed storage types
  let showStorageBadges = $derived.by(() => {
    const types = new Set(allLocalWorkspaces.map(w => getWorkspaceStorageType(w.id)));
    return types.size > 1;
  });

  function storageLabel(type: StorageType): string {
    switch (type) {
      case 'opfs': return 'OPFS';
      case 'indexeddb': return 'IDB';
      case 'filesystem-access': return 'Folder';
      default: return type;
    }
  }

  let showSelector = $derived(workspaces.length > 0);

  // Current workspace ID (reactive via auth state + local registry state)
  let authState = $derived(getAuthState());
  let currentWsId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());

  // Display name
  let displayName = $derived.by(() => {
    if (currentWsId) {
      const found = workspaces.find(w => w.id === currentWsId);
      if (found) return found.name;
    }
    const storedId = typeof localStorage !== 'undefined' ? localStorage.getItem('diaryx_current_workspace') : null;
    const localWs = storedId
      ? allLocalWorkspaces.find(w => w.id === storedId)
      : allLocalWorkspaces[0];
    return localWs?.name ?? 'My Journal';
  });

  // Fetch remote extras when popover opens
  async function onPopoverOpen() {
    const extras: Record<string, RemoteWorkspace[]> = {};
    const localServerIds = new Set(
      allLocalWorkspaces
        .map(w => getServerWorkspaceId(w.id))
        .filter((id): id is string => !!id),
    );

    for (const provider of workspaceProviders) {
      const status = await getProviderStatus(provider.contribution.id);
      if (!status.ready) continue;

      const unlinked = await listUnlinkedRemoteWorkspaces(
        provider.contribution.id,
        localServerIds,
      );
      if (unlinked.length > 0) {
        extras[provider.contribution.id] = unlinked;
      }
    }
    remoteExtras = extras;
  }

  $effect(() => {
    if (open) {
      onPopoverOpen();
    } else {
      // Reset inline states when popover closes
      menuOpenId = null;
      renamingId = null;
      confirmDeleteId = null;
    }
  });

  async function handleSelect(ws: LocalWorkspaceEntry) {
    if (currentWsId === ws.id) {
      open = false;
      return;
    }
    await doSwitch(ws.id, ws.name);
  }

  async function doSwitch(id: string, name: string) {
    switching = true;
    open = false;
    onSwitchStart?.();
    try {
      await switchWorkspace(id, name, {
        onTeardownComplete: () => {},
        onReady: () => {
          console.log("[WorkspaceSelector] Switch complete");
        },
      });
      onSwitchComplete?.();
    } catch (e) {
      console.error("[WorkspaceSelector] Switch failed:", e);
      if (e instanceof BackendError && e.kind === "WorkspaceDirectoryMissing") {
        onWorkspaceMissing?.({ id, name });
      } else {
        toast.error("Failed to switch workspace");
      }
    } finally {
      switching = false;
    }
  }

  function openRemotePicker(pluginId: string) {
    const provider = workspaceProviders.find(p => p.contribution.id === pluginId);
    if (!provider) return;
    const ws = remoteExtras[pluginId] ?? [];
    pickerProvider = { pluginId, label: provider.contribution.label, workspaces: ws };
  }

  async function handleDownloadRemote(remoteId: string, name: string) {
    downloading = remoteId;
    try {
      const { localId } = await downloadWorkspace(
        pickerProvider!.pluginId,
        { remoteId, name, link: true },
        () => {}, // progress not shown in selector
      );
      pickerProvider = null;
      open = false;
      await doSwitch(localId, name);
      toast.success("Workspace downloaded", { description: `"${name}" is ready.` });
    } catch (e) {
      console.error("[WorkspaceSelector] Download failed:", e);
      toast.error("Failed to download workspace");
    } finally {
      downloading = null;
    }
  }

  async function handleCreateWorkspace() {
    open = false;
    pickerProvider = null;
    await tick();
    onAddWorkspace?.();
  }

  // --- Rename state ---
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let renameLoading = $state(false);

  function startRename(ws: LocalWorkspaceEntry) {
    menuOpenId = null;
    renamingId = ws.id;
    renameValue = ws.name;
  }

  function cancelRename() {
    renamingId = null;
    renameValue = "";
  }

  async function submitRename() {
    if (!renamingId || !renameValue.trim()) return;
    renameLoading = true;
    try {
      const serverWorkspaces = getServerWorkspaces();
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

  // --- Delete state ---
  let confirmDeleteId = $state<string | null>(null);
  let deleteLoading = $state(false);

  function startDelete(ws: LocalWorkspaceEntry) {
    menuOpenId = null;
    confirmDeleteId = ws.id;
  }

  function cancelDelete() {
    confirmDeleteId = null;
  }

  async function confirmDelete(id: string) {
    deleteLoading = true;
    try {
      const ws = allLocalWorkspaces.find(w => w.id === id);
      await deleteLocalWorkspaceData(id, ws?.name);
      removeLocalWorkspace(id);
      toast.success("Workspace deleted");
      confirmDeleteId = null;
    } catch (e: any) {
      toast.error(e?.message || "Failed to delete workspace");
    } finally {
      deleteLoading = false;
    }
  }

  // --- Three-dot menu state ---
  let menuOpenId = $state<string | null>(null);
</script>

{#if showSelector}
  <Popover.Root bind:open>
    <Popover.Trigger>
      <button
        type="button"
        class="flex items-center gap-1.5 px-2 py-2.5 md:py-1 -mx-1 rounded-md text-sm font-medium text-sidebar-foreground/80 hover:bg-sidebar-accent hover:text-sidebar-foreground transition-colors max-w-[180px]"
        disabled={switching}
      >
        {#if switching}
          <Loader2 class="size-3.5 animate-spin shrink-0" />
        {/if}
        <span class="truncate">{displayName}</span>
        <ChevronsUpDown class="size-3.5 shrink-0 opacity-50" />
      </button>
    </Popover.Trigger>
    <Popover.Content class="w-64 p-0" align="start" side="bottom">
      {#if pickerProvider}
        <!-- Remote Workspace Picker -->
        <div class="p-2">
          <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
            {pickerProvider.label}
          </p>
        </div>
        <div class="max-h-64 overflow-y-auto">
          {#each pickerProvider.workspaces as ws (ws.id)}
            <button
              type="button"
              class="flex items-center gap-2 w-full px-4 py-2 text-sm text-left hover:bg-accent transition-colors"
              disabled={downloading === ws.id}
              onclick={() => handleDownloadRemote(ws.id, ws.name)}
            >
              <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
              <span class="truncate flex-1">{ws.name}</span>
              {#if downloading === ws.id}
                <Loader2 class="size-3.5 animate-spin shrink-0 text-muted-foreground" />
              {:else}
                <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">download</span>
              {/if}
            </button>
          {/each}
        </div>
        <div class="border-t p-2">
          <button
            type="button"
            class="flex items-center gap-2 w-full px-2 py-2.5 md:py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
            onclick={() => { pickerProvider = null; }}
          >
            Back
          </button>
        </div>
      {:else}
        <!-- Local Workspace List -->
        <div class="p-2">
          <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
            Workspaces
          </p>
        </div>
        <div class="max-h-64 overflow-y-auto">
          {#each workspaces as ws (ws.id)}
            {#if renamingId === ws.id}
              <!-- Inline rename row -->
              <div class="flex items-center gap-2 px-4 py-1.5">
                <Input
                  bind:value={renameValue}
                  onkeydown={handleRenameKeydown}
                  class="h-7 text-sm flex-1"
                  disabled={renameLoading}
                />
                <button
                  type="button"
                  class="size-11 md:size-6 flex items-center justify-center rounded-md hover:bg-accent transition-colors"
                  onclick={submitRename}
                  disabled={renameLoading || !renameValue.trim()}
                >
                  {#if renameLoading}
                    <Loader2 class="size-4 md:size-3.5 animate-spin" />
                  {:else}
                    <Check class="size-4 md:size-3.5" />
                  {/if}
                </button>
                <button
                  type="button"
                  class="size-11 md:size-6 flex items-center justify-center rounded-md hover:bg-accent transition-colors"
                  onclick={cancelRename}
                  disabled={renameLoading}
                >
                  <span class="text-xs">&#x2715;</span>
                </button>
              </div>
            {:else if confirmDeleteId === ws.id}
              <!-- Inline delete confirmation row -->
              <div class="flex items-center gap-2 px-4 py-1.5">
                <span class="text-sm text-destructive truncate flex-1">Delete "{ws.name}"?</span>
                <button
                  type="button"
                  class="px-3 py-2 md:px-2 md:py-0.5 text-xs rounded-md bg-destructive text-destructive-foreground hover:bg-destructive/90 transition-colors"
                  onclick={() => confirmDelete(ws.id)}
                  disabled={deleteLoading}
                >
                  {#if deleteLoading}
                    <Loader2 class="size-3 animate-spin" />
                  {:else}
                    Delete
                  {/if}
                </button>
                <button
                  type="button"
                  class="size-11 md:size-6 flex items-center justify-center rounded-md hover:bg-accent transition-colors"
                  onclick={cancelDelete}
                >
                  <span class="text-xs">&#x2715;</span>
                </button>
              </div>
            {:else}
              <!-- Normal workspace row -->
              <div class="group flex items-center gap-0 w-full hover:bg-accent transition-colors">
                <button
                  type="button"
                  class="flex items-center gap-2 flex-1 min-w-0 px-4 py-2 text-sm text-left"
                  disabled={switching}
                  onclick={() => handleSelect(ws)}
                >
                  <span class="size-4 shrink-0 flex items-center justify-center">
                    {#if currentWsId === ws.id}
                      <Check class="size-3.5" />
                    {/if}
                  </span>
                  {#if ws.synced}
                    <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                  {:else}
                    <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="truncate flex-1">{ws.name}</span>
                  {#if showStorageBadges}
                    <span class="text-[9px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0 font-medium">{storageLabel(getWorkspaceStorageType(ws.id))}</span>
                  {/if}
                </button>
                <!-- Three-dot menu -->
                <div class="relative pr-2">
                  <button
                    type="button"
                    class="size-11 md:size-6 flex items-center justify-center rounded-md opacity-0 group-hover:opacity-100 hover:bg-accent-foreground/10 transition-all {menuOpenId === ws.id ? 'opacity-100' : ''}"
                    onclick={(e) => { e.stopPropagation(); menuOpenId = menuOpenId === ws.id ? null : ws.id; }}
                  >
                    <Ellipsis class="size-4 md:size-3.5 text-muted-foreground" />
                  </button>
                  {#if menuOpenId === ws.id}
                    <div class="absolute right-0 top-7 z-50 min-w-[120px] rounded-md border bg-popover p-1 shadow-md">
                      <button
                        type="button"
                        class="flex items-center gap-2 w-full px-2 py-1.5 text-sm rounded-sm hover:bg-accent transition-colors"
                        onclick={(e) => { e.stopPropagation(); startRename(ws); }}
                      >
                        <Pencil class="size-3.5" />
                        Rename
                      </button>
                      <button
                        type="button"
                        class="flex items-center gap-2 w-full px-2 py-1.5 text-sm rounded-sm hover:bg-accent transition-colors text-destructive disabled:opacity-50"
                        disabled={currentWsId === ws.id}
                        title={currentWsId === ws.id ? "Switch to another workspace first" : "Delete workspace"}
                        onclick={(e) => { e.stopPropagation(); startDelete(ws); }}
                      >
                        <Trash2 class="size-3.5" />
                        Delete
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            {/if}
          {/each}
        </div>

        <!-- Footer: provider extras + create -->
        <div class="border-t">
          {#each Object.entries(remoteExtras) as [pluginId, extras] (pluginId)}
            {@const provider = workspaceProviders.find(p => p.contribution.id === pluginId)}
            {#if provider && extras.length > 0}
              <button
                type="button"
                class="flex items-center gap-2 w-full px-4 py-1.5 text-[10px] text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                onclick={() => openRemotePicker(pluginId)}
              >
                <Cloud class="size-3" />
                {extras.length} more on {provider.contribution.label}
              </button>
            {/if}
          {/each}
          <div class="p-2">
            <button
              type="button"
              class="flex items-center gap-2 w-full px-2 py-2.5 md:py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
              onclick={handleCreateWorkspace}
            >
              <Plus class="size-3.5" />
              New workspace
            </button>
          </div>
        </div>
      {/if}
    </Popover.Content>
  </Popover.Root>
{/if}
