<script lang="ts">
  import { tick } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
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

  // Derived state
  let allLocalWorkspaces = $derived(getLocalWorkspaces());

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

  let workspaceListEl: HTMLDivElement | null = $state(null);
  let savedWorkspaceListScrollTop = $state(0);
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let renameLoading = $state(false);
  let confirmDeleteId = $state<string | null>(null);
  let deleteLoading = $state(false);
  let menuOpenId = $state<string | null>(null);

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

  $effect(() => {
    if (!open) {
      // Reset inline states when popover closes
      menuOpenId = null;
      renamingId = null;
      confirmDeleteId = null;
    }
  });

  $effect.pre(() => {
    open;
    renamingId;
    confirmDeleteId;
    workspaces.length;
    if (open && workspaceListEl) {
      savedWorkspaceListScrollTop = workspaceListEl.scrollTop;
    }
  });

  $effect(() => {
    open;
    renamingId;
    confirmDeleteId;
    workspaces.length;
    void tick().then(() => {
      if (open && workspaceListEl && savedWorkspaceListScrollTop > 0) {
        workspaceListEl.scrollTop = savedWorkspaceListScrollTop;
      }
    });
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

  async function handleCreateWorkspace() {
    open = false;
    await tick();
    onAddWorkspace?.();
  }

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
</script>

{#if showSelector}
  <button
    type="button"
    class="flex items-center gap-1.5 px-2 py-2.5 md:py-1 -mx-1 rounded-md text-sm font-medium text-sidebar-foreground/80 hover:bg-sidebar-accent hover:text-sidebar-foreground transition-colors max-w-[180px]"
    disabled={switching}
    onclick={() => { open = true; }}
  >
    {#if switching}
      <Loader2 class="size-3.5 animate-spin shrink-0" />
    {/if}
    <span class="truncate">{displayName}</span>
    <ChevronsUpDown class="size-3.5 shrink-0 opacity-50" />
  </button>

  <Dialog.Root bind:open>
    <Dialog.Content class="sm:max-w-sm p-0 gap-0">
      <Dialog.Header class="p-4 pb-2">
        <Dialog.Title class="text-sm font-medium">Workspaces</Dialog.Title>
      </Dialog.Header>

      <div class="max-h-64 overflow-y-auto" data-workspace-selector-list bind:this={workspaceListEl}>
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
              <div class="pr-2">
                <DropdownMenu.Root bind:open={
                  () => menuOpenId === ws.id,
                  (v) => { menuOpenId = v ? ws.id : null; }
                }>
                  <DropdownMenu.Trigger
                    class="size-11 md:size-6 flex items-center justify-center rounded-md opacity-0 group-hover:opacity-100 hover:bg-accent-foreground/10 transition-all {menuOpenId === ws.id ? 'opacity-100' : ''}"
                    aria-label={"Workspace actions for " + ws.name}
                    onclick={(e: MouseEvent) => { e.stopPropagation(); }}
                  >
                    <Ellipsis class="size-4 md:size-3.5 text-muted-foreground" />
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content align="end" side="bottom" class="min-w-[120px]">
                    <DropdownMenu.Item
                      onclick={(e: Event) => { e.stopPropagation(); startRename(ws); }}
                    >
                      <Pencil class="size-3.5" />
                      Rename
                    </DropdownMenu.Item>
                    <DropdownMenu.Item
                      variant="destructive"
                      disabled={currentWsId === ws.id}
                      aria-label={"Delete workspace " + ws.name}
                      title={currentWsId === ws.id ? "Switch to another workspace first" : "Delete workspace"}
                      onclick={(e: Event) => { e.stopPropagation(); startDelete(ws); }}
                    >
                      <Trash2 class="size-3.5" />
                      Delete
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </div>
            </div>
          {/if}
        {/each}
      </div>

      <!-- Footer -->
      <div class="border-t p-2">
        <button
          type="button"
          class="flex items-center gap-2 w-full px-2 py-2.5 md:py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
          onclick={handleCreateWorkspace}
        >
          <Plus class="size-3.5" />
          New workspace
        </button>
      </div>
    </Dialog.Content>
  </Dialog.Root>
{/if}
