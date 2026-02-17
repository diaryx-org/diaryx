<script lang="ts">
  import * as Popover from "$lib/components/ui/popover";
  import { Button } from "$lib/components/ui/button";
  import {
    ChevronsUpDown,
    Check,
    Plus,
    Loader2,
    HardDrive,
    Cloud,
  } from "@lucide/svelte";
  import {
    getAuthState,
    getCurrentWorkspace,
    getWorkspaces,
    getWorkspaceLimit,
    createServerWorkspace,
    downloadWorkspaceSnapshot,
  } from "$lib/auth";
  import {
    isWorkspaceLocal,
    addLocalWorkspace,
    setCurrentWorkspaceId,
    getLocalWorkspaces,
    createLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { switchWorkspace } from "$lib/crdt/workspaceCrdtBridge";
  import { getBackend } from "$lib/backend";
  import { createApi } from "$lib/backend/api";
  import { toast } from "svelte-sonner";
  import type { Workspace } from "$lib/auth/authService";

  interface Props {
    onSwitchStart?: () => void;
    onSwitchComplete?: () => void;
  }

  let { onSwitchStart, onSwitchComplete }: Props = $props();

  let open = $state(false);
  let switching = $state(false);
  let creating = $state(false);
  let downloading = $state<string | null>(null);
  let newWorkspaceName = $state("");
  let showCreateInput = $state(false);

  // Derived state
  let authState = $derived(getAuthState());
  let currentWorkspace = $derived(getCurrentWorkspace());
  let serverWorkspaces = $derived(getWorkspaces());
  let workspaceLimit = $derived(getWorkspaceLimit());
  let allLocalWorkspaces = $derived(getLocalWorkspaces());

  // Merge server and local workspaces into a unified list.
  // When logged in: workspaces on server with isLocal=false are 'server', rest are 'local'.
  // When logged out: all workspaces are 'local' (no syncing possible).
  type UnifiedWorkspace = { id: string; name: string; source: 'server' | 'local' };
  let allWorkspaces = $derived.by(() => {
    const merged: UnifiedWorkspace[] = [];
    const seen = new Set<string>();

    if (authState.isAuthenticated) {
      // Server workspaces that aren't flagged local in registry
      for (const ws of serverWorkspaces) {
        const localEntry = allLocalWorkspaces.find(lw => lw.id === ws.id);
        const isLocalOnly = localEntry?.isLocal ?? false;
        merged.push({ id: ws.id, name: ws.name, source: isLocalOnly ? 'local' : 'server' });
        seen.add(ws.id);
      }
    }
    // All local-registry workspaces not already added
    for (const ws of allLocalWorkspaces) {
      if (!seen.has(ws.id)) {
        merged.push({ id: ws.id, name: ws.name, source: 'local' });
        seen.add(ws.id);
      }
    }
    return merged;
  });

  let syncedCount = $derived(serverWorkspaces.length);
  let localCount = $derived(allWorkspaces.filter(w => w.source === 'local').length);

  // Always show selector so users can create new workspaces
  let showSelector = $derived(allWorkspaces.length > 0);
  let canCreateServer = $derived(authState.isAuthenticated && serverWorkspaces.length < workspaceLimit);

  // Current workspace name for display
  let currentName = $derived.by(() => {
    if (currentWorkspace?.name) return currentWorkspace.name;
    const all = getLocalWorkspaces();
    const currentId = localStorage.getItem('diaryx_current_workspace');
    const localWs = currentId ? all.find(w => w.id === currentId) : all[0];
    return localWs?.name ?? 'My Journal';
  });

  // Current workspace ID (from reactive auth state, updated by switchWorkspace)
  let currentWsId = $derived(authState.activeWorkspaceId);

  function isLocal(id: string): boolean {
    return isWorkspaceLocal(id);
  }

  async function handleSelect(ws: UnifiedWorkspace) {
    if (currentWsId === ws.id) {
      open = false;
      return;
    }

    if (ws.source === 'local') {
      // Local workspace — switch directly
      await doSwitch(ws.id, ws.name);
      return;
    }

    if (!isLocal(ws.id)) {
      // Server workspace not downloaded — download first
      await handleDownloadAndSwitch(ws as Workspace);
      return;
    }

    // Switch to locally-available server workspace
    await doSwitch(ws.id, ws.name);
  }

  async function handleDownloadAndSwitch(ws: Workspace) {
    downloading = ws.id;
    try {
      // Download the snapshot
      const blob = await downloadWorkspaceSnapshot(ws.id, true);
      if (!blob) {
        toast.error("Failed to download workspace");
        return;
      }

      // Register locally
      addLocalWorkspace({ id: ws.id, name: ws.name });
      setCurrentWorkspaceId(ws.id);

      // Create backend for this workspace and import the snapshot
      const backend = await getBackend(ws.id, ws.name);
      const api = createApi(backend);

      // Create workspace structure
      try {
        await api.createWorkspace(".", ws.name);
      } catch {
        // May already exist
      }

      // Import the snapshot
      const file = new File([blob], "snapshot.zip", { type: "application/zip" });
      await backend.importFromZip(file);

      // Now switch to it
      await doSwitch(ws.id, ws.name);
    } catch (e) {
      console.error("[WorkspaceSelector] Download failed:", e);
      toast.error("Failed to download workspace");
    } finally {
      downloading = null;
    }
  }

  async function doSwitch(id: string, name: string) {
    switching = true;
    open = false;
    onSwitchStart?.();
    try {
      await switchWorkspace(id, name, {
        onTeardownComplete: () => {
          // UI state clearing handled by App.svelte via onSwitchStart
        },
        onReady: () => {
          console.log("[WorkspaceSelector] Switch complete");
        },
      });
      onSwitchComplete?.();
    } catch (e) {
      console.error("[WorkspaceSelector] Switch failed:", e);
      toast.error("Failed to switch workspace");
    } finally {
      switching = false;
    }
  }

  async function handleCreate() {
    const name = newWorkspaceName.trim();
    if (!name) return;

    creating = true;
    try {
      if (authState.isAuthenticated && canCreateServer) {
        // Create on server (synced)
        const ws = await createServerWorkspace(name);
        newWorkspaceName = "";
        showCreateInput = false;
        toast.success(`Workspace "${name}" created`);

        // Immediately switch to the new workspace
        addLocalWorkspace({ id: ws.id, name: ws.name });
        await doSwitch(ws.id, ws.name);
      } else {
        // Create local-only workspace (no limit)
        const ws = createLocalWorkspace(name);
        newWorkspaceName = "";
        showCreateInput = false;
        toast.success(`Workspace "${name}" created`);
        await doSwitch(ws.id, ws.name);
      }
    } catch (e: any) {
      if (e?.statusCode === 403) {
        toast.error("Synced workspace limit reached");
      } else if (e?.statusCode === 409) {
        toast.error("A workspace with that name already exists");
      } else {
        toast.error("Failed to create workspace");
      }
    } finally {
      creating = false;
    }
  }

  function handleCreateKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      handleCreate();
    } else if (e.key === "Escape") {
      showCreateInput = false;
      newWorkspaceName = "";
    }
  }
</script>

{#if showSelector}
  <Popover.Root bind:open>
    <Popover.Trigger>
      <button
        type="button"
        class="flex items-center gap-1.5 px-2 py-1 -mx-1 rounded-md text-sm font-medium text-sidebar-foreground/80 hover:bg-sidebar-accent hover:text-sidebar-foreground transition-colors max-w-[180px]"
        disabled={switching}
      >
        {#if switching}
          <Loader2 class="size-3.5 animate-spin shrink-0" />
        {/if}
        <span class="truncate">{currentWorkspace?.name ?? currentName}</span>
        <ChevronsUpDown class="size-3.5 shrink-0 opacity-50" />
      </button>
    </Popover.Trigger>
    <Popover.Content class="w-64 p-0" align="start" side="bottom">
      <div class="p-2">
        <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
          Workspaces
        </p>
      </div>
      <div class="max-h-64 overflow-y-auto">
        {#each allWorkspaces as ws (ws.id)}
          <button
            type="button"
            class="flex items-center gap-2 w-full px-4 py-2 text-sm text-left hover:bg-accent transition-colors"
            disabled={switching || downloading === ws.id}
            onclick={() => handleSelect(ws)}
          >
            <span class="size-4 shrink-0 flex items-center justify-center">
              {#if currentWsId === ws.id}
                <Check class="size-3.5" />
              {/if}
            </span>
            <!-- Icon indicating local vs synced -->
            {#if ws.source === 'local'}
              <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
            {:else}
              <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
            {/if}
            <span class="truncate flex-1">{ws.name}</span>
            {#if downloading === ws.id}
              <Loader2 class="size-3.5 animate-spin shrink-0 text-muted-foreground" />
            {:else if ws.source === 'server' && !isLocal(ws.id)}
              <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">cloud</span>
            {/if}
          </button>
        {/each}
      </div>

      <!-- Footer: counts + create -->
      <div class="border-t">
        {#if authState.isAuthenticated}
          <div class="px-4 py-1.5 text-[10px] text-muted-foreground flex items-center gap-3">
            <span class="flex items-center gap-1"><Cloud class="size-3" /> {syncedCount}/{workspaceLimit} synced</span>
            {#if localCount > 0}
              <span class="flex items-center gap-1"><HardDrive class="size-3" /> {localCount} local</span>
            {/if}
          </div>
        {/if}
        <div class="p-2 {authState.isAuthenticated ? '' : 'pt-2'}">
          {#if showCreateInput}
            <div class="flex items-center gap-1">
              <input
                type="text"
                bind:value={newWorkspaceName}
                onkeydown={handleCreateKeydown}
                placeholder="Workspace name"
                class="flex-1 px-2 py-1 text-sm border rounded-md bg-background"
                disabled={creating}
              />
              <Button
                variant="ghost"
                size="sm"
                onclick={handleCreate}
                disabled={creating || !newWorkspaceName.trim()}
                class="h-7 px-2"
              >
                {#if creating}
                  <Loader2 class="size-3.5 animate-spin" />
                {:else}
                  Add
                {/if}
              </Button>
            </div>
            {#if authState.isAuthenticated && canCreateServer}
              <p class="text-[10px] text-muted-foreground mt-1 px-1">Creates a synced workspace</p>
            {:else if authState.isAuthenticated}
              <p class="text-[10px] text-muted-foreground mt-1 px-1">Creates a local workspace (synced limit reached)</p>
            {/if}
          {:else}
            <button
              type="button"
              class="flex items-center gap-2 w-full px-2 py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
              onclick={() => { showCreateInput = true; }}
            >
              <Plus class="size-3.5" />
              New workspace
            </button>
          {/if}
        </div>
      </div>
    </Popover.Content>
  </Popover.Root>
{/if}
