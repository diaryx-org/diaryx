<script lang="ts">
  import { tick } from "svelte";
  import * as Popover from "$lib/components/ui/popover";
  import {
    ChevronsUpDown,
    Check,
    Plus,
    Loader2,
    HardDrive,
    Cloud,
  } from "@lucide/svelte";
  import {
    getServerWorkspaceId,
    getLocalWorkspaces,
    getWorkspaceStorageType,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import type { StorageType } from "$lib/backend/storageType";
  import { switchWorkspace } from "$lib/crdt/workspaceCrdtBridge";
  import { getAuthState } from "$lib/auth";
  import { toast } from "svelte-sonner";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getProviderStatus,
    listUnlinkedRemoteWorkspaces,
    downloadWorkspace,
    type RemoteWorkspace,
  } from "$lib/sync/workspaceProviderService";

  interface Props {
    onSwitchStart?: () => void;
    onSwitchComplete?: () => void;
    onAddWorkspace?: () => void;
  }

  let { onSwitchStart, onSwitchComplete, onAddWorkspace }: Props = $props();

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

  // Current workspace ID (reactive via auth state, updated by switchWorkspace)
  let authState = $derived(getAuthState());
  let currentWsId = $derived(authState.activeWorkspaceId);

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
      const status = getProviderStatus(provider.contribution.id);
      if (!status.ready) continue;

      const unlinked = listUnlinkedRemoteWorkspaces(
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
      toast.error("Failed to switch workspace");
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
            class="flex items-center gap-2 w-full px-2 py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
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
            <button
              type="button"
              class="flex items-center gap-2 w-full px-4 py-2 text-sm text-left hover:bg-accent transition-colors"
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
              class="flex items-center gap-2 w-full px-2 py-1 text-sm text-muted-foreground hover:text-foreground hover:bg-accent rounded-md transition-colors"
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
