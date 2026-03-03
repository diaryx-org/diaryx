<script lang="ts">
  /**
   * PermissionBanner — shows pending plugin permission requests at the top of the editor.
   *
   * Primary actions: Allow (this time), Deny (this time)
   * More options dropdown: Allow file permanently, Allow folder permanently, Block permanently
   */
  import { permissionStore } from '@/models/stores';
  import type { PermissionRequest } from '@/models/stores/permissionStore.svelte';
  import { ChevronDown, Shield, X } from '@lucide/svelte';

  let showMoreOptions = $state<string | null>(null);

  function handleAllow(request: PermissionRequest) {
    permissionStore.resolveRequest(request.id, true);
  }

  function handleDeny(request: PermissionRequest) {
    permissionStore.resolveRequest(request.id, false);
  }

  function handleDismiss(request: PermissionRequest) {
    permissionStore.dismissRequest(request.id);
  }

  function toggleMoreOptions(requestId: string) {
    showMoreOptions = showMoreOptions === requestId ? null : requestId;
  }

  function formatRequestMessage(request: PermissionRequest): string {
    const action = permissionStore.getPermissionLabel(request.permissionType);
    const target = permissionStore.formatTarget(request.permissionType, request.target);
    return `${request.pluginName} wants to ${action} ${target}`;
  }
</script>

{#if permissionStore.hasPendingRequests}
  <div class="flex flex-col gap-1 border-b border-border bg-muted/50">
    {#each permissionStore.pendingRequests as request (request.id)}
      <div class="flex items-center gap-3 px-4 py-2">
        <Shield class="size-4 shrink-0 text-amber-500" />
        <span class="flex-1 text-sm text-foreground truncate">
          {formatRequestMessage(request)}
        </span>
        <div class="flex items-center gap-1.5 shrink-0">
          <button
            class="rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
            onclick={() => handleAllow(request)}
          >
            Allow
          </button>
          <button
            class="rounded-md bg-secondary px-3 py-1 text-xs font-medium text-secondary-foreground hover:bg-secondary/80 transition-colors"
            onclick={() => handleDeny(request)}
          >
            Deny
          </button>
          <div class="relative">
            <button
              class="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
              onclick={() => toggleMoreOptions(request.id)}
              title="More options"
            >
              <ChevronDown class="size-3.5" />
            </button>
            {#if showMoreOptions === request.id}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="absolute right-0 top-full z-50 mt-1 w-56 rounded-md border border-border bg-popover p-1 shadow-md"
                onclick={(e) => e.stopPropagation()}
              >
                <button
                  class="flex w-full items-center rounded-sm px-2 py-1.5 text-xs hover:bg-accent hover:text-accent-foreground transition-colors"
                  onclick={() => {
                    void permissionStore.persistRequestDecision(request.id, "allow_target");
                    showMoreOptions = null;
                  }}
                >
                  Allow this file permanently
                </button>
                <button
                  class="flex w-full items-center rounded-sm px-2 py-1.5 text-xs hover:bg-accent hover:text-accent-foreground transition-colors"
                  onclick={() => {
                    void permissionStore.persistRequestDecision(request.id, "allow_folder");
                    showMoreOptions = null;
                  }}
                >
                  Allow folder permanently
                </button>
                <div class="my-1 h-px bg-border"></div>
                <button
                  class="flex w-full items-center rounded-sm px-2 py-1.5 text-xs text-destructive hover:bg-destructive/10 transition-colors"
                  onclick={() => {
                    void permissionStore.persistRequestDecision(request.id, "block_target");
                    showMoreOptions = null;
                  }}
                >
                  Block permanently
                </button>
              </div>
            {/if}
          </div>
          <button
            class="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
            onclick={() => handleDismiss(request)}
            title="Dismiss"
          >
            <X class="size-3.5" />
          </button>
        </div>
      </div>
    {/each}
  </div>
{/if}
