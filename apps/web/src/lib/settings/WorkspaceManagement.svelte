<script lang="ts">
  /**
   * WorkspaceManagement - Read-only workspace overview from settings.
   */
  import { Separator } from "$lib/components/ui/separator";
  import {
    HardDrive,
    Cloud,
    CloudDownload,
  } from "@lucide/svelte";
  import { getAuthState, listUserWorkspaceNamespaces } from "$lib/auth";
  import {
    getLocalWorkspaces,
    getCurrentWorkspaceId,
    getWorkspaceProviderLinks,
    isWorkspaceSyncEnabled,
  } from "$lib/storage/localWorkspaceRegistry.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getProviderStatus, listRemoteWorkspaces, type RemoteWorkspace } from "$lib/sync/workspaceProviderService";
  import {
    getProviderDisplayLabel,
    getProviderUnavailableReason,
    isProviderAvailableHere,
  } from "$lib/sync/builtinProviders";
  import type { NamespaceEntry } from "$lib/auth/authService";

  const pluginStore = getPluginStore();

  let authState = $derived(getAuthState());
  let currentId = $derived(authState.activeWorkspaceId ?? getCurrentWorkspaceId());
  let allLocal = $derived(getLocalWorkspaces());
  let workspaceProviders = $derived(pluginStore.workspaceProviders);

  let syncedWorkspaces = $derived.by(() =>
    allLocal.filter((workspace) => getWorkspaceProviderLinks(workspace.id).length > 0),
  );
  let localWorkspaces = $derived.by(() =>
    allLocal.filter((workspace) => getWorkspaceProviderLinks(workspace.id).length === 0),
  );
  let hasAnyWorkspaces = $derived(
    syncedWorkspaces.length > 0 || localWorkspaces.length > 0,
  );

  let cloudWorkspacesByProvider = $state<Record<string, RemoteWorkspace[]>>({});
  let accountWorkspaceNamespaces = $state<NamespaceEntry[]>([]);
  let providerReadyById = $state<Record<string, boolean>>({});

  let linkedRemoteKeys = $derived.by(() => {
    const keys = new Set<string>();
    for (const ws of allLocal) {
      for (const link of getWorkspaceProviderLinks(ws.id)) {
        keys.add(`${link.pluginId}:${link.remoteWorkspaceId}`);
      }
    }
    return keys;
  });

  let unavailableCloudNamespaces = $derived.by(() =>
    accountWorkspaceNamespaces.filter((ns) => {
      const providerId = namespaceProviderId(ns);
      return !isProviderAvailableHere(providerId)
        && !linkedRemoteKeys.has(`${providerId}:${ns.id}`);
    }),
  );

  let readyWorkspaceProviders = $derived.by(() =>
    workspaceProviders.filter((provider) => providerReadyById[provider.contribution.id]),
  );

  let cloudProviderSections = $derived.by(() =>
    readyWorkspaceProviders
      .map((provider) => ({
        providerId: provider.contribution.id,
        label: provider.contribution.label,
        workspaces: cloudWorkspacesByProvider[provider.contribution.id] ?? [],
      }))
      .filter((entry) => entry.workspaces.length > 0),
  );

  $effect(() => {
    if (workspaceProviders.length === 0) {
      providerReadyById = {};
      return;
    }

    void (async () => {
      const nextStatuses: Record<string, boolean> = {};
      await Promise.all(
        workspaceProviders.map(async (provider) => {
          try {
            const status = await getProviderStatus(provider.contribution.id);
            nextStatuses[provider.contribution.id] = status.ready;
          } catch {
            nextStatuses[provider.contribution.id] = false;
          }
        }),
      );
      providerReadyById = nextStatuses;
    })();
  });

  $effect(() => {
    if (!authState.isAuthenticated) {
      accountWorkspaceNamespaces = [];
      return;
    }

    void (async () => {
      try {
        accountWorkspaceNamespaces = await listUserWorkspaceNamespaces();
      } catch {
        accountWorkspaceNamespaces = [];
      }
    })();
  });

  $effect(() => {
    if (readyWorkspaceProviders.length === 0) {
      cloudWorkspacesByProvider = {};
      return;
    }

    void (async () => {
      const nextByProvider: Record<string, RemoteWorkspace[]> = {};
      await Promise.all(
        readyWorkspaceProviders.map(async (provider) => {
          try {
            const localRemoteIds = new Set(
              syncedWorkspaces
                .map((ws) => getWorkspaceProviderLinks(ws.id)
                  .find((link) => link.pluginId === provider.contribution.id)?.remoteWorkspaceId)
                .filter((id): id is string => !!id),
            );
            nextByProvider[provider.contribution.id] = await listRemoteWorkspaces(provider.contribution.id)
              .then((workspaces) => workspaces.filter((workspace) => !localRemoteIds.has(workspace.id)));
          } catch {
            nextByProvider[provider.contribution.id] = [];
          }
        }),
      );
      cloudWorkspacesByProvider = nextByProvider;
    })();
  });

  function namespaceProviderId(ns: NamespaceEntry): string {
    return ns.metadata?.provider ?? "diaryx.sync";
  }

  function linkedProviderLabel(workspaceId: string): string | null {
    const providerId = getWorkspaceProviderLinks(workspaceId)[0]?.pluginId;
    if (!providerId) return null;
    return getProviderDisplayLabel(providerId)
      ?? workspaceProviders.find((provider) => provider.contribution.id === providerId)?.contribution.label
      ?? providerId;
  }

  function namespaceProviderLabel(ns: NamespaceEntry): string {
    const providerId = namespaceProviderId(ns);
    return getProviderDisplayLabel(providerId) ?? providerId;
  }

  function namespaceUnavailableReason(ns: NamespaceEntry): string | null {
    return getProviderUnavailableReason(namespaceProviderId(ns));
  }

  function workspaceName(ns: NamespaceEntry): string {
    return ns.metadata?.name ?? ns.id;
  }
</script>

{#if hasAnyWorkspaces}
  <div class="space-y-4">
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
              <span class="flex items-center gap-1.5 flex-1 min-w-0">
                <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                <span class="text-sm truncate">{ws.name}</span>
                <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">
                  {isWorkspaceSyncEnabled(ws.id) ? "sync enabled" : "publish only"}
                </span>
                {#if linkedProviderLabel(ws.id)}
                  <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">
                    via {linkedProviderLabel(ws.id)}
                  </span>
                {/if}
                {#if ws.id === currentId}
                  <span class="text-[10px] px-1 py-0.5 rounded bg-primary/10 text-primary shrink-0">active</span>
                {/if}
              </span>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if cloudProviderSections.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <CloudDownload class="size-3.5 text-muted-foreground" />
            Cloud Workspaces
          </h3>
          <span class="text-xs text-muted-foreground">
            {cloudProviderSections.reduce((total, section) => total + section.workspaces.length, 0)}
          </span>
        </div>

        <Separator />

        <div class="space-y-1">
          {#each cloudProviderSections as section (section.providerId)}
            {#each section.workspaces as remote (remote.id)}
              <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-secondary">
                <span class="flex items-center gap-1.5 flex-1 min-w-0">
                  <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                  <span class="min-w-0 flex-1">
                    <span class="text-sm truncate block">{remote.name}</span>
                    <span class="text-[10px] text-muted-foreground block">
                      via {section.label}
                    </span>
                  </span>
                  <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">cloud only</span>
                </span>
              </div>
            {/each}
          {/each}
        </div>

        <p class="text-xs text-muted-foreground">
          These workspaces exist on the account but are not on this device.
        </p>
      </div>
    {/if}

    {#if unavailableCloudNamespaces.length > 0}
      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium flex items-center gap-1.5">
            <Cloud class="size-3.5 text-muted-foreground" />
            Unavailable Cloud Workspaces
          </h3>
          <span class="text-xs text-muted-foreground">
            {unavailableCloudNamespaces.length}
          </span>
        </div>

        <Separator />

        <div class="space-y-1">
          {#each unavailableCloudNamespaces as ns (ns.id)}
            <div class="flex items-center gap-2 py-1.5 px-2 rounded-md border border-dashed bg-secondary/30">
              <span class="flex items-center gap-1.5 flex-1 min-w-0">
                <Cloud class="size-3.5 shrink-0 text-muted-foreground" />
                <span class="min-w-0 flex-1">
                  <span class="text-sm truncate block">{workspaceName(ns)}</span>
                  <span class="text-[10px] text-muted-foreground block">
                    via {namespaceProviderLabel(ns)}
                  </span>
                  {#if namespaceUnavailableReason(ns)}
                    <span class="text-[10px] text-muted-foreground block mt-0.5">
                      {namespaceUnavailableReason(ns)}
                    </span>
                  {/if}
                </span>
                <span class="text-[10px] px-1 py-0.5 rounded bg-muted text-muted-foreground shrink-0">
                  unavailable
                </span>
              </span>
            </div>
          {/each}
        </div>

        <p class="text-xs text-muted-foreground">
          These workspaces are linked to your account but cannot be opened on this device.
        </p>
      </div>
    {/if}

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
    {/if}

    {#if syncedWorkspaces.length > 0 || localWorkspaces.length > 0}
      <p class="text-xs text-muted-foreground">
        Change where a workspace lives from the welcome screen’s storage flow.
      </p>
    {/if}
  </div>
{/if}
