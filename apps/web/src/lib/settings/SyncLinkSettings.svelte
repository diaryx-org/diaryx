<script lang="ts">
  /**
   * SyncLinkSettings — per-workspace remote link management.
   *
   * Shows the current provider link if one exists, or allows linking
   * to an available remote workspace namespace.
   */
  import { Button } from "$lib/components/ui/button";
  import { Cloud, Loader2, Unlink, Link2 } from "@lucide/svelte";
  import { toast } from "svelte-sonner";
  import {
    getPrimaryWorkspaceProviderLink,
    type WorkspaceProviderLink,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import {
    getAuthState,
    listUserWorkspaceNamespaces,
    type NamespaceEntry,
  } from "$lib/auth";
  import {
    linkWorkspace,
    unlinkWorkspace,
  } from "$lib/sync/workspaceProviderService";
  import {
    getProviderDisplayLabel,
    isProviderAvailableHere,
  } from "$lib/sync/builtinProviders";

  interface Props {
    workspaceId: string;
    workspaceName: string;
  }

  let { workspaceId, workspaceName }: Props = $props();

  let authState = $derived(getAuthState());
  let providerLink = $derived(getPrimaryWorkspaceProviderLink(workspaceId));

  // Remote namespace picker state
  let showPicker = $state(false);
  let availableNamespaces = $state<NamespaceEntry[]>([]);
  let loadingNamespaces = $state(false);
  let linking = $state(false);
  let unlinking = $state(false);

  function providerLabel(link: WorkspaceProviderLink): string {
    return getProviderDisplayLabel(link.pluginId) ?? link.pluginId;
  }

  async function handleShowPicker() {
    showPicker = true;
    loadingNamespaces = true;
    try {
      const allNamespaces = await listUserWorkspaceNamespaces();
      // Filter out namespaces already linked to any local workspace
      const allLinks = new Set<string>();
      // We only need to check if it's linked — providerLink for THIS workspace
      // is shown separately. Exclude all linked remote IDs across workspaces.
      const { getLocalWorkspaces, getWorkspaceProviderLinks: getLinks } = await import(
        "$lib/storage/localWorkspaceRegistry.svelte"
      );
      for (const ws of getLocalWorkspaces()) {
        for (const link of getLinks(ws.id)) {
          allLinks.add(link.remoteWorkspaceId);
        }
      }
      availableNamespaces = allNamespaces.filter(
        (ns) => !allLinks.has(ns.id) && isProviderAvailableHere(ns.metadata?.provider ?? "diaryx.sync"),
      );
    } catch {
      availableNamespaces = [];
    } finally {
      loadingNamespaces = false;
    }
  }

  async function handleLink(ns: NamespaceEntry) {
    const providerId = ns.metadata?.provider ?? "diaryx.sync";
    linking = true;
    try {
      await linkWorkspace(providerId, {
        localId: workspaceId,
        name: workspaceName,
        remoteId: ns.id,
      });
      showPicker = false;
      toast.success("Workspace linked", {
        description: `Linked to "${ns.metadata?.name ?? ns.id}".`,
      });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to link workspace");
    } finally {
      linking = false;
    }
  }

  async function handleUnlink() {
    if (!providerLink) return;
    unlinking = true;
    try {
      await unlinkWorkspace(providerLink.pluginId, workspaceId);
      toast.success("Workspace unlinked");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to unlink workspace");
    } finally {
      unlinking = false;
    }
  }
</script>

<div class="space-y-3">
  <h3 class="font-medium flex items-center gap-2">
    <Cloud class="size-4" />
    Remote Link
  </h3>

  {#if providerLink}
    <div class="flex items-start gap-3 p-3 rounded-lg border border-border mx-1">
      <Cloud class="size-5 text-muted-foreground mt-0.5 shrink-0" />
      <div class="flex-1 min-w-0">
        <div class="font-medium text-sm">Linked</div>
        <p class="text-xs text-muted-foreground mt-0.5">
          via {providerLabel(providerLink)}
        </p>
        {#if providerLink.syncEnabled}
          <p class="text-xs text-emerald-600 dark:text-emerald-400 mt-0.5">Sync enabled</p>
        {/if}
      </div>
    </div>
    <Button
      variant="outline"
      size="sm"
      class="w-full mx-1"
      onclick={handleUnlink}
      disabled={unlinking}
    >
      {#if unlinking}
        <Loader2 class="size-3.5 mr-1.5 animate-spin" />
        Unlinking...
      {:else}
        <Unlink class="size-3.5 mr-1.5" />
        Unlink from remote
      {/if}
    </Button>
  {:else if !authState.isAuthenticated}
    <p class="text-xs text-muted-foreground px-1">
      Sign in to link this workspace to a remote workspace.
    </p>
  {:else if showPicker}
    {#if loadingNamespaces}
      <div class="flex items-center gap-2 text-xs text-muted-foreground px-1 py-2">
        <Loader2 class="size-3.5 animate-spin" />
        Loading remote workspaces...
      </div>
    {:else if availableNamespaces.length === 0}
      <p class="text-xs text-muted-foreground px-1">
        No unlinked remote workspaces available.
      </p>
      <Button variant="ghost" size="sm" onclick={() => { showPicker = false; }}>
        Cancel
      </Button>
    {:else}
      <div class="space-y-1 max-h-48 overflow-y-auto px-1">
        {#each availableNamespaces as ns (ns.id)}
          <button
            type="button"
            class="flex items-center gap-2 w-full px-3 py-2 rounded-md text-left hover:bg-accent transition-colors disabled:opacity-60"
            disabled={linking}
            onclick={() => handleLink(ns)}
          >
            <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
            <span class="text-sm truncate flex-1">{ns.metadata?.name ?? ns.id}</span>
            {#if linking}
              <Loader2 class="size-3 animate-spin shrink-0" />
            {/if}
          </button>
        {/each}
      </div>
      <Button variant="ghost" size="sm" onclick={() => { showPicker = false; }}>
        Cancel
      </Button>
    {/if}
  {:else}
    <p class="text-xs text-muted-foreground px-1">
      This workspace is local only.
    </p>
    <Button
      variant="outline"
      size="sm"
      class="w-full"
      onclick={handleShowPicker}
    >
      <Link2 class="size-3.5 mr-1.5" />
      Link to remote workspace
    </Button>
  {/if}
</div>
