<script lang="ts">
  import * as Popover from "$lib/components/ui/popover";
  import { Button } from "$lib/components/ui/button";
  import {
    ChevronsUpDown,
    Check,
    Plus,
    Loader2,
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
  } from "$lib/storage/localWorkspaceRegistry";
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
  let canCreate = $derived(serverWorkspaces.length < workspaceLimit);

  function isLocal(id: string): boolean {
    return isWorkspaceLocal(id);
  }

  function isCurrent(id: string): boolean {
    return currentWorkspace?.id === id;
  }

  async function handleSelect(ws: Workspace) {
    if (isCurrent(ws.id)) {
      open = false;
      return;
    }

    if (!isLocal(ws.id)) {
      // Need to download first
      await handleDownloadAndSwitch(ws);
      return;
    }

    // Switch to locally-available workspace
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
      const backend = await getBackend(ws.id);
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
      const ws = await createServerWorkspace(name);
      newWorkspaceName = "";
      showCreateInput = false;
      toast.success(`Workspace "${name}" created`);

      // Immediately switch to the new workspace
      addLocalWorkspace({ id: ws.id, name: ws.name });
      await doSwitch(ws.id, ws.name);
    } catch (e: any) {
      if (e?.statusCode === 403) {
        toast.error("Workspace limit reached");
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

{#if authState.isAuthenticated && serverWorkspaces.length > 0}
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
        <span class="truncate">{currentWorkspace?.name ?? "Workspace"}</span>
        <ChevronsUpDown class="size-3.5 shrink-0 opacity-50" />
      </button>
    </Popover.Trigger>
    <Popover.Content class="w-64 p-0" align="start" side="bottom">
      <div class="p-2">
        <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
          Workspaces ({serverWorkspaces.length}/{workspaceLimit})
        </p>
      </div>
      <div class="max-h-64 overflow-y-auto">
        {#each serverWorkspaces as ws (ws.id)}
          <button
            type="button"
            class="flex items-center gap-2 w-full px-4 py-2 text-sm text-left hover:bg-accent transition-colors"
            disabled={switching || downloading === ws.id}
            onclick={() => handleSelect(ws)}
          >
            <span class="size-4 shrink-0 flex items-center justify-center">
              {#if isCurrent(ws.id)}
                <Check class="size-3.5" />
              {/if}
            </span>
            <span class="truncate flex-1">{ws.name}</span>
            {#if downloading === ws.id}
              <Loader2 class="size-3.5 animate-spin shrink-0 text-muted-foreground" />
            {:else if !isLocal(ws.id)}
              <span class="text-xs text-muted-foreground">cloud</span>
            {/if}
          </button>
        {/each}
      </div>
      {#if canCreate}
        <div class="border-t p-2">
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
      {/if}
    </Popover.Content>
  </Popover.Root>
{/if}
