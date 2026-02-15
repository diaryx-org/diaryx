<script lang="ts">
  /**
   * WorkspaceManagement - Manage server workspaces from settings.
   *
   * Features:
   * - List all server workspaces with local/cloud status
   * - Rename workspaces
   * - Delete workspaces (with confirmation)
   * - Shows workspace limit usage
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
  } from "@lucide/svelte";
  import {
    getAuthState,
    getWorkspaces,
    getWorkspaceLimit,
    renameServerWorkspace,
    deleteServerWorkspace,
  } from "$lib/auth";
  import {
    isWorkspaceLocal,
    removeLocalWorkspace,
    renameLocalWorkspace,
  } from "$lib/storage/localWorkspaceRegistry";
  import { toast } from "svelte-sonner";

  let authState = $derived(getAuthState());
  let workspaces = $derived(getWorkspaces());
  let workspaceLimit = $derived(getWorkspaceLimit());
  let currentId = $derived(authState.activeWorkspaceId);

  // Rename state
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let renameLoading = $state(false);

  // Delete state
  let deletingId = $state<string | null>(null);
  let deleteLoading = $state(false);
  let confirmDeleteId = $state<string | null>(null);

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
      await renameServerWorkspace(renamingId, renameValue.trim());
      renameLocalWorkspace(renamingId, renameValue.trim());
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

  async function handleDelete(id: string) {
    if (confirmDeleteId !== id) {
      confirmDeleteId = id;
      return;
    }

    deleteLoading = true;
    deletingId = id;
    try {
      await deleteServerWorkspace(id);
      removeLocalWorkspace(id);
      toast.success("Workspace deleted");
      confirmDeleteId = null;
    } catch (e: any) {
      toast.error(e?.message || "Failed to delete workspace");
    } finally {
      deleteLoading = false;
      deletingId = null;
    }
  }

  function cancelDelete() {
    confirmDeleteId = null;
  }
</script>

{#if authState.isAuthenticated && workspaces.length > 0}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-medium">Workspaces</h3>
      <span class="text-xs text-muted-foreground">
        {workspaces.length} / {workspaceLimit}
      </span>
    </div>

    <Separator />

    <div class="space-y-1">
      {#each workspaces as ws (ws.id)}
        <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-muted/50 group">
          {#if renamingId === ws.id}
            <!-- Rename mode -->
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
          {:else}
            <!-- Display mode -->
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
              <Button
                variant="ghost"
                size="icon"
                class="size-6"
                onclick={() => startRename(ws.id, ws.name)}
                title="Rename"
              >
                <Pencil class="size-3" />
              </Button>
              {#if ws.id !== currentId}
                {#if confirmDeleteId === ws.id}
                  <Button
                    variant="destructive"
                    size="sm"
                    class="h-6 text-xs"
                    onclick={() => handleDelete(ws.id)}
                    disabled={deleteLoading}
                  >
                    {#if deleteLoading && deletingId === ws.id}
                      <Loader2 class="size-3 animate-spin mr-1" />
                    {/if}
                    Confirm
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-6"
                    onclick={cancelDelete}
                  >
                    <X class="size-3" />
                  </Button>
                {:else}
                  <Button
                    variant="ghost"
                    size="icon"
                    class="size-6"
                    onclick={() => handleDelete(ws.id)}
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

    {#if isWorkspaceLocal(currentId ?? '')}
      <p class="text-xs text-muted-foreground">
        The active workspace cannot be deleted. Switch to a different workspace first.
      </p>
    {/if}
  </div>
{/if}
