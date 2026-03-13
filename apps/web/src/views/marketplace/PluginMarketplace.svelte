<script lang="ts">
  import {
    Store,
    Search,
    Loader2,
    Download,
    Upload,
    Trash2,
    Check,
    X,
    ExternalLink,
    ArrowLeft,
    SlidersHorizontal,
  } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Switch } from "$lib/components/ui/switch";
  import { Badge } from "$lib/components/ui/badge";
  import { Separator } from "$lib/components/ui/separator";
  import { toast } from "svelte-sonner";
  import {
    fetchPluginRegistry,
    type RegistryPlugin,
  } from "$lib/plugins/pluginRegistry";
  import {
    getBrowserPluginSupport,
    getBrowserPluginSupportError,
    installPlugin as browserInstallPlugin,
    uninstallPlugin as browserUninstallPlugin,
    inspectPluginWasm,
  } from "$lib/plugins/browserPluginManager.svelte";
  import { getBackend, isTauri } from "$lib/backend";
  import { createApi } from "$lib/backend/api";
  import type { Backend } from "$lib/backend/interface";
  import type {
    PermissionType,
    PluginConfig,
    PluginPermissions,
  } from "@/models/stores/permissionStore.svelte";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { openExternalUrl } from "$lib/billing";

  interface Props {
    onClose: () => void;
  }

  let { onClose }: Props = $props();

  let closing = $state(false);

  function handleClose() {
    closing = true;
    setTimeout(onClose, 200);
  }

  const pluginStore = getPluginStore();
  const mobileState = getMobileState();

  let registryPlugins = $state<RegistryPlugin[]>([]);
  let registryLoading = $state(true);
  let registryError = $state<string | null>(null);
  let installingIds = $state<Set<string>>(new Set());
  let removingIds = $state<Set<string>>(new Set());
  let uploadingLocal = $state(false);
  let fileInputRef = $state<HTMLInputElement | null>(null);

  let search = $state("");
  let categoryFilter = $state("all");
  let capabilityFilter = $state("all");
  let sourceFilter = $state<"all" | "installed">("all");
  let sortBy = $state<"name" | "version" | "recent">("name");
  let selectedPluginId = $state<string | null>(null);
  let showDetail = $state(false);
  let filtersOpen = $state(false);

  function selectPlugin(id: string) {
    selectedPluginId = id;
    if (mobileState.isMobile) showDetail = true;
  }

  const browserPluginSupport = $derived(getBrowserPluginSupport());
  const browserPluginSupportError = $derived(getBrowserPluginSupportError());
  const pluginsSupported = $derived(isTauri() || browserPluginSupport.supported);

  const installedIds = $derived(
    new Set(pluginStore.allManifests.map((m) => String(m.id))),
  );

  const localPlugins = $derived.by(() => {
    const registryIds = new Set(registryPlugins.map((r) => r.id));
    return pluginStore.allManifests
      .filter((m) => !registryIds.has(String(m.id)))
      .sort((a, b) => String(a.name ?? a.id).localeCompare(String(b.name ?? b.id)));
  });

  const categories = $derived.by(() => {
    const all = new Set<string>();
    for (const plugin of registryPlugins) {
      for (const category of plugin.categories) all.add(category);
    }
    return ["all", ...Array.from(all).sort()];
  });

  const capabilities = $derived.by(() => {
    const all = new Set<string>();
    for (const plugin of registryPlugins) {
      for (const capability of plugin.capabilities) all.add(capability);
    }
    return ["all", ...Array.from(all).sort()];
  });

  const filteredPlugins = $derived.by(() => {
    const query = search.trim().toLowerCase();

    const filtered = registryPlugins.filter((plugin) => {
      if (sourceFilter === "installed" && !installedIds.has(plugin.id)) return false;

      if (categoryFilter !== "all" && !plugin.categories.includes(categoryFilter)) {
        return false;
      }
      if (capabilityFilter !== "all" && !plugin.capabilities.includes(capabilityFilter)) {
        return false;
      }

      if (!query) return true;
      const haystack = [
        plugin.id,
        plugin.name,
        plugin.summary,
        plugin.description,
        plugin.author,
        plugin.license,
        ...plugin.tags,
        ...plugin.categories,
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query);
    });

    filtered.sort((a, b) => {
      if (sortBy === "name") {
        return a.name.localeCompare(b.name);
      }
      if (sortBy === "version") {
        return b.version.localeCompare(a.version);
      }
      const aTs = Date.parse(a.artifact.published_at) || 0;
      const bTs = Date.parse(b.artifact.published_at) || 0;
      return bTs - aTs;
    });

    return filtered;
  });

  const selectedPlugin = $derived.by(() => {
    if (filteredPlugins.length === 0) return null;
    const explicit = filteredPlugins.find((p) => p.id === selectedPluginId);
    return explicit ?? filteredPlugins[0] ?? null;
  });

  $effect(() => {
    if (!selectedPluginId && filteredPlugins.length > 0) {
      selectedPluginId = filteredPlugins[0].id;
    }
    if (
      selectedPluginId &&
      filteredPlugins.length > 0 &&
      !filteredPlugins.some((plugin) => plugin.id === selectedPluginId)
    ) {
      selectedPluginId = filteredPlugins[0].id;
    }
  });

  async function loadRegistry() {
    registryLoading = true;
    registryError = null;
    try {
      const registry = await fetchPluginRegistry();
      registryPlugins = registry.plugins;
    } catch (e) {
      registryError =
        e instanceof Error ? e.message : "Failed to load plugin registry";
      registryPlugins = [];
    } finally {
      registryLoading = false;
    }
  }

  $effect(() => {
    loadRegistry();
  });

  async function platformInstall(
    bytes: ArrayBuffer,
    name?: string,
    expectedPluginId?: string,
  ): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.installPlugin) {
        const manifestJson = await backend.installPlugin(new Uint8Array(bytes));
        if (expectedPluginId) {
          let installedId: string | null = null;
          try {
            const parsed = JSON.parse(manifestJson);
            if (typeof parsed?.id === "string") {
              installedId = parsed.id;
            }
          } catch {
            // Keep installedId as null and fail below.
          }
          if (!installedId) {
            throw new Error("Installed plugin manifest did not include a valid plugin ID.");
          }
          if (installedId !== expectedPluginId) {
            throw new Error(
              `Installed plugin ID mismatch: expected '${expectedPluginId}', got '${installedId}'`,
            );
          }
        }
        return;
      }
    }
    await browserInstallPlugin(bytes, name);
  }

  async function platformUninstall(pluginId: string): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.uninstallPlugin) {
        await backend.uninstallPlugin(pluginId);
        return;
      }
    }
    await browserUninstallPlugin(pluginId);
  }

  function normalizeSha256(value: string): string {
    return value.trim().toLowerCase().replace(/^sha256:/, "");
  }

  async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
    if (typeof crypto === "undefined" || !crypto.subtle) {
      throw new Error("SHA-256 verification is unavailable in this runtime.");
    }
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    const arr = Array.from(new Uint8Array(digest));
    return arr.map((b) => b.toString(16).padStart(2, "0")).join("");
  }

  async function verifyRegistryArtifact(bytes: ArrayBuffer, expectedSha: string): Promise<void> {
    const actual = await sha256Hex(bytes);
    if (actual !== normalizeSha256(expectedSha)) {
      throw new Error("Plugin integrity check failed (SHA-256 mismatch)");
    }
  }

  const PERMISSION_LABELS: Record<PermissionType, string> = {
    read_files: "Read files",
    edit_files: "Edit files",
    create_files: "Create files",
    delete_files: "Delete files",
    move_files: "Move files",
    http_requests: "HTTP requests",
    execute_commands: "Execute commands",
    plugin_storage: "Plugin storage",
  };

  function formatRuleSummary(permissionType: PermissionType, rule: { include: string[]; exclude: string[] }): string {
    if (permissionType === "plugin_storage") return "all";
    if (!rule.include?.length) return "no includes";
    return rule.include.join(", ");
  }

  async function persistDefaultPermissions(pluginId: string, defaults: PluginPermissions): Promise<void> {
    const rootIndexPath = workspaceStore.tree?.path;
    if (!rootIndexPath) return;

    const backend = await getBackend();
    const api = createApi(backend);
    const fm = await api.getFrontmatter(rootIndexPath);
    const existingPlugins = (fm.plugins as Record<string, PluginConfig> | undefined) ?? {};
    const existingPluginConfig = existingPlugins[pluginId] ?? { permissions: {} };
    const mergedPermissions: PluginPermissions = {
      ...(existingPluginConfig.permissions ?? {}),
    };

    for (const [permissionType, requestedRule] of Object.entries(defaults)) {
      if (!requestedRule) continue;
      if (!mergedPermissions[permissionType as PermissionType]) {
        mergedPermissions[permissionType as PermissionType] = {
          include: [...(requestedRule.include ?? [])],
          exclude: [...(requestedRule.exclude ?? [])],
        };
      }
    }

    const nextPlugins: Record<string, PluginConfig> = {
      ...existingPlugins,
      [pluginId]: {
        ...existingPluginConfig,
        permissions: mergedPermissions,
      },
    };

    await api.setFrontmatterProperty(rootIndexPath, "plugins", nextPlugins as any, rootIndexPath);
  }

  async function reviewAndInstall(
    bytes: ArrayBuffer,
    fallbackName?: string,
    expectedPluginId?: string,
  ): Promise<void> {
    if (isTauri()) {
      await platformInstall(bytes, fallbackName, expectedPluginId);
      return;
    }

    const inspected = await inspectPluginWasm(bytes);
    const pluginId = inspected.pluginId;
    if (expectedPluginId && pluginId !== expectedPluginId) {
      throw new Error(
        `Plugin ID mismatch: expected '${expectedPluginId}', got '${pluginId}'`,
      );
    }
    const pluginName = inspected.pluginName || fallbackName || pluginId;
    const requested = inspected.requestedPermissions;
    const defaults = requested?.defaults ?? {};
    const reasons = requested?.reasons ?? {};

    const requestedLines = Object.entries(defaults)
      .filter(([, rule]) => !!rule)
      .map(([permissionType, rule]) => {
        const typed = permissionType as PermissionType;
        const reason = reasons[typed];
        const summary = formatRuleSummary(typed, rule!);
        if (reason) {
          return `- ${PERMISSION_LABELS[typed]}: ${summary}\n  Why: ${reason}`;
        }
        return `- ${PERMISSION_LABELS[typed]}: ${summary}`;
      });

    const details =
      requestedLines.length > 0
        ? requestedLines.join("\n")
        : "- This plugin requests no default permissions.";

    const approved = window.confirm(
      `Install "${pluginName}" (${pluginId})?\n\n` +
        `Requested default permissions:\n${details}\n\n` +
        `Approved defaults will be saved in root frontmatter under plugins.${pluginId}.permissions.`,
    );
    if (!approved) return;

    if (requested?.defaults) {
      await persistDefaultPermissions(pluginId, requested.defaults);
    }

    await platformInstall(bytes, fallbackName ?? pluginName, expectedPluginId);
  }

  async function installFromRegistry(plugin: RegistryPlugin): Promise<void> {
    installingIds = new Set([...installingIds, plugin.id]);
    try {
      const response = await fetch(plugin.artifact.url);
      if (!response.ok) {
        throw new Error(`Download failed: ${response.status}`);
      }
      const bytes = await response.arrayBuffer();
      await verifyRegistryArtifact(bytes, plugin.artifact.sha256);
      await reviewAndInstall(bytes, plugin.name, plugin.id);
      toast.success(`Installed ${plugin.name}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : `Failed to install ${plugin.name}`);
    } finally {
      installingIds = new Set([...installingIds].filter((id) => id !== plugin.id));
    }
  }

  async function removePlugin(pluginId: string, pluginName: string): Promise<void> {
    removingIds = new Set([...removingIds, pluginId]);
    try {
      await platformUninstall(pluginId);
      toast.success(`Removed ${pluginName}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : `Failed to remove ${pluginName}`);
    } finally {
      removingIds = new Set([...removingIds].filter((id) => id !== pluginId));
    }
  }

  function triggerUpload(): void {
    fileInputRef?.click();
  }

  async function onLocalFileSelected(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";

    if (!file.name.endsWith(".wasm")) {
      toast.error("Please select a .wasm file");
      return;
    }

    uploadingLocal = true;
    try {
      const bytes = await file.arrayBuffer();
      await reviewAndInstall(bytes, file.name.replace(/\.wasm$/, ""));
      toast.success(`Installed local plugin from ${file.name}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to install plugin");
    } finally {
      uploadingLocal = false;
    }
  }

  function setEnabled(pluginId: string, enabled: boolean): void {
    pluginStore.setPluginEnabled(pluginId, enabled);
  }
</script>

<div class="fixed inset-0 z-50 bg-background overflow-hidden {closing ? 'animate-marketplace-out' : 'animate-marketplace-in'}">
  <div class="h-full flex flex-col">
    <header class="border-b px-4 py-3 flex items-center justify-between gap-3 pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.75rem)] shrink-0">
      <div class="flex items-center gap-2 min-w-0">
        <Store class="size-5 shrink-0" />
        <div>
          <h2 class="text-lg font-semibold">Plugin Marketplace</h2>
          <p class="text-xs text-muted-foreground hidden sm:block">Curated registries, signed metadata, SHA-256 verified installs.</p>
        </div>
      </div>
      <div class="flex items-center gap-2 shrink-0">
        <Button variant="outline" size="sm" onclick={triggerUpload} disabled={uploadingLocal || !pluginsSupported}>
          {#if uploadingLocal}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />
            Installing...
          {:else}
            <Upload class="size-3.5 mr-1.5" />
            <span class="hidden sm:inline">Add Local</span>
            <span class="sm:hidden">Add</span>
          {/if}
        </Button>
        <Button variant="ghost" size="icon" onclick={handleClose} aria-label="Close marketplace">
          <X class="size-4" />
        </Button>
      </div>
      <input
        type="file"
        accept=".wasm"
        class="hidden"
        bind:this={fileInputRef}
        onchange={onLocalFileSelected}
      />
    </header>

    {#if !pluginsSupported}
      <div class="px-4 pt-3 shrink-0">
        <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-3 text-xs text-amber-700 dark:text-amber-300">
          {browserPluginSupportError ?? browserPluginSupport.reason ?? "Browser plugins are unavailable in this browser."}
        </div>
      </div>
    {/if}

    <div class="px-4 pt-3 pb-2 flex flex-nowrap items-center gap-2 border-b shrink-0 overflow-x-auto">
      <div class="relative flex-1 min-w-0">
        <Search class="size-4 absolute left-2.5 top-2.5 text-muted-foreground" />
        <Input class="pl-8" placeholder="Search plugins" bind:value={search} />
      </div>

      {#if mobileState.isMobile}
        <!-- Mobile: toggle filters -->
        <Button
          variant="outline"
          size="icon"
          onclick={() => (filtersOpen = !filtersOpen)}
          aria-label="Toggle filters"
          class={filtersOpen ? 'border-primary' : ''}
        >
          <SlidersHorizontal class="size-4" />
        </Button>
      {/if}

      {#if !mobileState.isMobile || filtersOpen}
        <div class="{mobileState.isMobile ? 'w-full flex flex-col gap-2 pt-2' : 'contents'}">
          <select class="h-9 rounded-md border bg-background px-2 text-sm shrink-0" bind:value={categoryFilter}>
            {#each categories as category}
              <option value={category}>{category === "all" ? "All categories" : category}</option>
            {/each}
          </select>

          <select class="h-9 rounded-md border bg-background px-2 text-sm shrink-0" bind:value={capabilityFilter}>
            {#each capabilities as capability}
              <option value={capability}>{capability === "all" ? "All capabilities" : capability}</option>
            {/each}
          </select>

          <select class="h-9 rounded-md border bg-background px-2 text-sm shrink-0" bind:value={sourceFilter}>
            <option value="all">All plugins</option>
            <option value="installed">Installed</option>
          </select>

          <select class="h-9 rounded-md border bg-background px-2 text-sm shrink-0" bind:value={sortBy}>
            <option value="name">Sort: Name</option>
            <option value="recent">Sort: Recent</option>
            <option value="version">Sort: Version</option>
          </select>
        </div>
      {/if}
    </div>

    <div class="flex-1 min-h-0 grid grid-rows-[minmax(0,1fr)] grid-cols-1 lg:grid-cols-[minmax(0,1fr)_420px]">
      <section class="min-h-0 overflow-auto {mobileState.isMobile ? '' : 'border-r'} pb-[env(safe-area-inset-bottom)]">
        {#if registryLoading}
          <div class="h-full flex items-center justify-center text-muted-foreground gap-2">
            <Loader2 class="size-4 animate-spin" />
            Loading plugin registry...
          </div>
        {:else if registryError}
          <div class="p-4 text-sm text-muted-foreground">{registryError}</div>
        {:else if filteredPlugins.length === 0}
          <div class="p-4 text-sm text-muted-foreground">No plugins match your filters.</div>
        {:else}
          <div class="grid gap-3 p-3 {mobileState.isMobile ? 'grid-cols-1' : 'md:grid-cols-2'}">
            {#each filteredPlugins as plugin}
              {@const installed = installedIds.has(plugin.id)}
              {@const installing = installingIds.has(plugin.id)}
              <button
                type="button"
                class={`text-left rounded-lg border p-3 transition ${selectedPlugin?.id === plugin.id && !mobileState.isMobile ? "border-primary" : "hover:border-foreground/40"}`}
                onclick={() => selectPlugin(plugin.id)}
              >
                <div class="flex items-center justify-between gap-2">
                  <h3 class="font-medium truncate">{plugin.name}</h3>
                  <Badge variant="secondary" class="text-[10px]">v{plugin.version}</Badge>
                </div>
                <p class="text-xs text-muted-foreground mt-1 line-clamp-2">{plugin.summary}</p>
                <div class="mt-2 flex flex-wrap gap-1">
                  <Badge variant="outline" class="text-[10px]">{plugin.author}</Badge>
                </div>
                <div class="mt-3 flex items-center justify-between gap-2">
                  {#if installed}
                    <span class="text-xs text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1">
                      <Check class="size-3" />
                      Installed
                    </span>
                  {:else}
                    <span class="text-xs text-muted-foreground">Not installed</span>
                  {/if}
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={(e) => {
                      e.stopPropagation();
                      void installFromRegistry(plugin);
                    }}
                    disabled={installing || installed || !pluginsSupported}
                  >
                    {#if installing}
                      <Loader2 class="size-3.5 mr-1.5 animate-spin" />
                      Installing...
                    {:else}
                      <Download class="size-3.5 mr-1.5" />
                      Install
                    {/if}
                  </Button>
                </div>
              </button>
            {/each}
          </div>
        {/if}

        <Separator />

        <div class="p-3 space-y-2">
          <h3 class="text-sm font-medium text-muted-foreground">Local Plugins (Unmanaged)</h3>
          {#if localPlugins.length === 0}
            <p class="text-xs text-muted-foreground">No local plugins installed.</p>
          {:else}
            <div class="space-y-2">
              {#each localPlugins as plugin}
                {@const pluginId = String(plugin.id)}
                {@const removing = removingIds.has(pluginId)}
                <div class="rounded-md border p-3 flex items-center justify-between gap-3">
                  <div class="min-w-0">
                    <p class="text-sm font-medium truncate">{plugin.name}</p>
                    <p class="text-xs text-muted-foreground truncate">{plugin.description || pluginId}</p>
                  </div>
                  <div class="flex items-center gap-2 shrink-0">
                    <Switch
                      id={`local-plugin-enabled-${pluginId}`}
                      checked={pluginStore.isPluginEnabled(pluginId)}
                      onCheckedChange={(checked) => setEnabled(pluginId, checked)}
                      disabled={!pluginsSupported}
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onclick={() => removePlugin(pluginId, String(plugin.name ?? pluginId))}
                      disabled={removing}
                    >
                      {#if removing}
                        <Loader2 class="size-3.5 animate-spin" />
                      {:else}
                        <Trash2 class="size-3.5 text-destructive" />
                      {/if}
                    </Button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <!-- Desktop detail panel -->
      {#if !mobileState.isMobile}
        <aside class="min-h-0 overflow-auto p-4 pb-[env(safe-area-inset-bottom)]">
          {#if selectedPlugin}
            {@render pluginDetailContent(selectedPlugin)}
          {:else}
            <p class="text-sm text-muted-foreground">Select a plugin to view details.</p>
          {/if}
        </aside>
      {/if}
    </div>

    <!-- Mobile slide-over detail panel -->
    {#if mobileState.isMobile && showDetail && selectedPlugin}
      <div class="fixed inset-0 z-[60] bg-background animate-detail-in flex flex-col">
        <header class="border-b px-4 py-3 flex items-center gap-3 pt-[calc(env(safe-area-inset-top)+var(--titlebar-area-height)+0.75rem)] shrink-0">
          <Button variant="ghost" size="icon" onclick={() => (showDetail = false)} aria-label="Back">
            <ArrowLeft class="size-4" />
          </Button>
          <h2 class="text-lg font-semibold truncate">{selectedPlugin.name}</h2>
        </header>
        <div class="flex-1 overflow-auto p-4 pb-[calc(env(safe-area-inset-bottom)+1rem)]">
          {@render pluginDetailContent(selectedPlugin)}
        </div>
      </div>
    {/if}
  </div>
</div>

{#snippet pluginDetailContent(plugin: RegistryPlugin)}
  {@const installed = installedIds.has(plugin.id)}
  {@const installing = installingIds.has(plugin.id)}
  {@const removing = removingIds.has(plugin.id)}
  <div class="space-y-4">
    <div>
      <div class="flex items-center gap-2 flex-wrap">
        <h3 class="text-lg font-semibold">{plugin.name}</h3>
        <Badge variant="secondary">v{plugin.version}</Badge>
      </div>
      <p class="text-sm text-muted-foreground mt-1">{plugin.summary}</p>
      <p class="text-sm mt-2">{plugin.description}</p>
    </div>

    <div class="grid grid-cols-2 gap-2 text-xs">
      <div class="rounded-md border p-2">
        <p class="text-muted-foreground">Author</p>
        <p class="font-medium">{plugin.author}</p>
      </div>
      <div class="rounded-md border p-2">
        <p class="text-muted-foreground">License</p>
        <p class="font-medium">{plugin.license}</p>
      </div>
      <div class="rounded-md border p-2">
        <p class="text-muted-foreground">Artifact Size</p>
        <p class="font-medium">{Math.round(plugin.artifact.size / 1024)} KB</p>
      </div>
      <div class="rounded-md border p-2">
        <p class="text-muted-foreground">Published</p>
        <p class="font-medium">{new Date(plugin.artifact.published_at).toLocaleDateString()}</p>
      </div>
    </div>

    <div class="space-y-2">
      <h4 class="text-sm font-medium">Capabilities</h4>
      <div class="flex flex-wrap gap-1">
        {#if plugin.capabilities.length === 0}
          <span class="text-xs text-muted-foreground">No declared capabilities.</span>
        {:else}
          {#each plugin.capabilities as capability}
            <Badge variant="outline" class="text-[11px]">{capability}</Badge>
          {/each}
        {/if}
      </div>
    </div>

    <div class="space-y-2">
      <h4 class="text-sm font-medium">Tags</h4>
      <div class="flex flex-wrap gap-1">
        {#if plugin.tags.length === 0}
          <span class="text-xs text-muted-foreground">No tags.</span>
        {:else}
          {#each plugin.tags as tag}
            <Badge variant="secondary" class="text-[11px]">{tag}</Badge>
          {/each}
        {/if}
      </div>
    </div>

    <div class="space-y-1 text-xs">
      {#if plugin.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-primary hover:underline"
          onclick={() => openExternalUrl(plugin.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}
    </div>

    <div class="rounded-md border p-2 text-xs">
      <p class="font-medium mb-1">Requested Permissions</p>
      {#if plugin.requested_permissions}
        <pre class="whitespace-pre-wrap break-words text-[11px] text-muted-foreground">{JSON.stringify(plugin.requested_permissions, null, 2)}</pre>
      {:else}
        <p class="text-muted-foreground">No explicit requested permissions in manifest.</p>
      {/if}
    </div>

    <div class="flex items-center gap-2 pt-1">
      {#if installed}
        <Switch
          id={`marketplace-plugin-enabled-${plugin.id}`}
          checked={pluginStore.isPluginEnabled(plugin.id)}
          onCheckedChange={(checked) => setEnabled(plugin.id, checked)}
          disabled={!pluginsSupported}
        />
        <Button
          variant="outline"
          size="sm"
          onclick={() => removePlugin(plugin.id, plugin.name)}
          disabled={removing}
        >
          {#if removing}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />
            Removing...
          {:else}
            <Trash2 class="size-3.5 mr-1.5" />
            Uninstall
          {/if}
        </Button>
      {:else}
        <Button
          size="sm"
          onclick={() => installFromRegistry(plugin)}
          disabled={installing || !pluginsSupported}
        >
          {#if installing}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />
            Installing...
          {:else}
            <Download class="size-3.5 mr-1.5" />
            Install
          {/if}
        </Button>
      {/if}
    </div>
  </div>
{/snippet}

<style>
  @keyframes marketplace-in {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  :global(.animate-marketplace-in) {
    animation: marketplace-in 0.25s ease-out;
  }

  @keyframes marketplace-out {
    from {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
    to {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
  }

  :global(.animate-marketplace-out) {
    animation: marketplace-out 0.2s ease-in forwards;
  }

  @keyframes detail-slide-in {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  :global(.animate-detail-in) {
    animation: detail-slide-in 0.2s ease-out;
  }
</style>
