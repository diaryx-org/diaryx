<script lang="ts">
  /**
   * WorkspaceManagement - Read-only local workspace overview from settings.
   */
  import { Separator } from "$lib/components/ui/separator";
  import { HardDrive } from "@lucide/svelte";
  import { getAuthState } from "$lib/auth";
  import {
    getLocalWorkspaces,
    getCurrentWorkspaceId,
  } from "$lib/storage/localWorkspaceRegistry.svelte";

  let authState = $derived(getAuthState());
  let currentId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());
  let localWorkspaces = $derived(getLocalWorkspaces());
  let hasAnyWorkspaces = $derived(localWorkspaces.length > 0);
</script>

{#if hasAnyWorkspaces}
  <div class="space-y-4">
    <div class="space-y-3">
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium flex items-center gap-1.5">
          <HardDrive class="size-3.5 text-muted-foreground" />
          Workspaces
        </h3>
        <span class="text-xs text-muted-foreground">
          {localWorkspaces.length}
        </span>
      </div>

      <Separator />

      <div class="space-y-1">
        {#each localWorkspaces as ws (ws.id)}
          <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-secondary">
            <span class="flex items-center gap-1.5 flex-1 min-w-0">
              <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
              <span class="text-sm truncate">{ws.name}</span>
              {#if ws.id === currentId}
                <span class="text-[10px] px-1 py-0.5 rounded bg-primary/10 text-primary shrink-0">active</span>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    </div>

    <p class="text-xs text-muted-foreground">
      Diaryx workspaces are local folders. Put the folder in iCloud Drive,
      Dropbox, Syncthing, Git, or another external sync tool if you want it
      available elsewhere.
    </p>
  </div>
{/if}
