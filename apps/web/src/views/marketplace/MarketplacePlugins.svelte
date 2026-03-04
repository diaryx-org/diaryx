<script lang="ts">
  import {
    Search,
    Loader2,
    Download,
    Upload,
    Trash2,
    Check,
    ArrowLeft,
    SlidersHorizontal,
    ExternalLink,
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
  import { openExternalUrl } from "$lib/billing";

  const pluginStore = getPluginStore();

  let registryPlugins = $state<RegistryPlugin[]>([]);
  let registryLoading = $state(true);
  let registryError = $state<string | null>(null);
  let installingIds = $state<Set<string>>(new Set());
  let removingIds = $state<Set<string>>(new Set());
  let uploadingLocal = $state(false);
  let fileInputRef = $state<HTMLInputElement | null>(null);

  let search = $state("");
  let filtersOpen = $state(false);
  let categoryFilter = $state("all");
  let capabilityFilter = $state("all");
  let sourceFilter = $state<"all" | "installed">("all");
  let sortBy = $state<"name" | "version" | "recent">("name");
  let detailPlugin = $state<RegistryPlugin | null>(null);

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
      if (categoryFilter !== "all" && !plugin.categories.includes(categoryFilter)) return false;
      if (capabilityFilter !== "all" && !plugin.capabilities.includes(capabilityFilter)) return false;

      if (!query) return true;
      const haystack = [
        plugin.id, plugin.name, plugin.summary, plugin.description,
        plugin.author, plugin.license, ...plugin.tags, ...plugin.categories,
      ].join(" ").toLowerCase();
      return haystack.includes(query);
    });

    filtered.sort((a, b) => {
      if (sortBy === "name") return a.name.localeCompare(b.name);
      if (sortBy === "version") return b.version.localeCompare(a.version);
      const aTs = Date.parse(a.artifact.published_at) || 0;
      const bTs = Date.parse(b.artifact.published_at) || 0;
      return bTs - aTs;
    });

    return filtered;
  });

  $effect(() => {
    loadRegistry();
  });

  async function loadRegistry() {
    registryLoading = true;
    registryError = null;
    try {
      const registry = await fetchPluginRegistry();
      registryPlugins = registry.plugins;
    } catch (e) {
      registryError = e instanceof Error ? e.message : "Failed to load plugin registry";
      registryPlugins = [];
    } finally {
      registryLoading = false;
    }
  }

  // --- Platform install/uninstall (same logic as PluginMarketplace) ---

  async function platformInstall(bytes: ArrayBuffer, name?: string, expectedPluginId?: string): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.installPlugin) {
        const manifestJson = await backend.installPlugin(new Uint8Array(bytes));
        if (expectedPluginId) {
          let installedId: string | null = null;
          try { const parsed = JSON.parse(manifestJson); if (typeof parsed?.id === "string") installedId = parsed.id; } catch {}
          if (!installedId) throw new Error("Installed plugin manifest did not include a valid plugin ID.");
          if (installedId !== expectedPluginId) throw new Error(`Installed plugin ID mismatch: expected '${expectedPluginId}', got '${installedId}'`);
        }
        return;
      }
    }
    await browserInstallPlugin(bytes, name);
  }

  async function platformUninstall(pluginId: string): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.uninstallPlugin) { await backend.uninstallPlugin(pluginId); return; }
    }
    await browserUninstallPlugin(pluginId);
  }

  function normalizeSha256(value: string): string {
    return value.trim().toLowerCase().replace(/^sha256:/, "");
  }

  async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
    if (typeof crypto === "undefined" || !crypto.subtle) throw new Error("SHA-256 verification is unavailable in this runtime.");
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return Array.from(new Uint8Array(digest)).map((b) => b.toString(16).padStart(2, "0")).join("");
  }

  async function verifyRegistryArtifact(bytes: ArrayBuffer, expectedSha: string): Promise<void> {
    const actual = await sha256Hex(bytes);
    if (actual !== normalizeSha256(expectedSha)) throw new Error("Plugin integrity check failed (SHA-256 mismatch)");
  }

  const PERMISSION_LABELS: Record<PermissionType, string> = {
    read_files: "Read files", edit_files: "Edit files", create_files: "Create files",
    delete_files: "Delete files", move_files: "Move files", http_requests: "HTTP requests",
    execute_commands: "Execute commands", plugin_storage: "Plugin storage",
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
    const mergedPermissions: PluginPermissions = { ...(existingPluginConfig.permissions ?? {}) };

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
      [pluginId]: { ...existingPluginConfig, permissions: mergedPermissions },
    };
    await api.setFrontmatterProperty(rootIndexPath, "plugins", nextPlugins as any, rootIndexPath);
  }

  async function reviewAndInstall(bytes: ArrayBuffer, fallbackName?: string, expectedPluginId?: string): Promise<void> {
    if (isTauri()) { await platformInstall(bytes, fallbackName, expectedPluginId); return; }
    const inspected = await inspectPluginWasm(bytes);
    const pluginId = inspected.pluginId;
    if (expectedPluginId && pluginId !== expectedPluginId) throw new Error(`Plugin ID mismatch: expected '${expectedPluginId}', got '${pluginId}'`);
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
        return reason ? `- ${PERMISSION_LABELS[typed]}: ${summary}\n  Why: ${reason}` : `- ${PERMISSION_LABELS[typed]}: ${summary}`;
      });

    const details = requestedLines.length > 0 ? requestedLines.join("\n") : "- This plugin requests no default permissions.";
    const approved = window.confirm(
      `Install "${pluginName}" (${pluginId})?\n\nRequested default permissions:\n${details}\n\nApproved defaults will be saved in root frontmatter under plugins.${pluginId}.permissions.`,
    );
    if (!approved) return;

    if (requested?.defaults) await persistDefaultPermissions(pluginId, requested.defaults);
    await platformInstall(bytes, fallbackName ?? pluginName, expectedPluginId);
  }

  async function installFromRegistry(plugin: RegistryPlugin): Promise<void> {
    installingIds = new Set([...installingIds, plugin.id]);
    try {
      const response = await fetch(plugin.artifact.url);
      if (!response.ok) throw new Error(`Download failed: ${response.status}`);
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
    if (!file.name.endsWith(".wasm")) { toast.error("Please select a .wasm file"); return; }

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

<input type="file" accept=".wasm" class="hidden" bind:this={fileInputRef} onchange={onLocalFileSelected} />

{#if detailPlugin}
  <!-- Detail drill-down view -->
  {@const plugin = detailPlugin}
  {@const installed = installedIds.has(plugin.id)}
  {@const installing = installingIds.has(plugin.id)}
  {@const removing = removingIds.has(plugin.id)}
  <div class="flex flex-col h-full">
    <div class="flex items-center gap-2 px-3 py-2 border-b shrink-0">
      <Button variant="ghost" size="icon" class="size-7" onclick={() => (detailPlugin = null)} aria-label="Back">
        <ArrowLeft class="size-4" />
      </Button>
      <h3 class="text-sm font-medium truncate">{plugin.name}</h3>
    </div>
    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
      <div>
        <div class="flex items-center gap-2 flex-wrap">
          <Badge variant="secondary">v{plugin.version}</Badge>
        </div>
        <p class="text-xs text-muted-foreground mt-1">{plugin.summary}</p>
        <p class="text-xs mt-2">{plugin.description}</p>
      </div>

      <div class="grid grid-cols-2 gap-1.5 text-[11px]">
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Author</p>
          <p class="font-medium">{plugin.author}</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">License</p>
          <p class="font-medium">{plugin.license}</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Size</p>
          <p class="font-medium">{Math.round(plugin.artifact.size / 1024)} KB</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Published</p>
          <p class="font-medium">{new Date(plugin.artifact.published_at).toLocaleDateString()}</p>
        </div>
      </div>

      {#if plugin.capabilities.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Capabilities</h4>
          <div class="flex flex-wrap gap-1">
            {#each plugin.capabilities as capability}
              <Badge variant="outline" class="text-[10px]">{capability}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if plugin.tags.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Tags</h4>
          <div class="flex flex-wrap gap-1">
            {#each plugin.tags as tag}
              <Badge variant="secondary" class="text-[10px]">{tag}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if plugin.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          onclick={() => openExternalUrl(plugin.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}

      {#if plugin.requested_permissions}
        <div class="rounded-md border p-2 text-[11px]">
          <p class="font-medium mb-1">Requested Permissions</p>
          <pre class="whitespace-pre-wrap break-words text-muted-foreground">{JSON.stringify(plugin.requested_permissions, null, 2)}</pre>
        </div>
      {/if}

      <div class="flex items-center gap-2 pt-1">
        {#if installed}
          <Switch
            id={`mp-detail-enabled-${plugin.id}`}
            checked={pluginStore.isPluginEnabled(plugin.id)}
            onCheckedChange={(checked) => setEnabled(plugin.id, checked)}
            disabled={!pluginsSupported}
          />
          <Button variant="outline" size="sm" onclick={() => removePlugin(plugin.id, plugin.name)} disabled={removing}>
            {#if removing}
              <Loader2 class="size-3.5 mr-1.5 animate-spin" />Removing...
            {:else}
              <Trash2 class="size-3.5 mr-1.5" />Uninstall
            {/if}
          </Button>
        {:else}
          <Button size="sm" onclick={() => installFromRegistry(plugin)} disabled={installing || !pluginsSupported}>
            {#if installing}
              <Loader2 class="size-3.5 mr-1.5 animate-spin" />Installing...
            {:else}
              <Download class="size-3.5 mr-1.5" />Install
            {/if}
          </Button>
        {/if}
      </div>
    </div>
  </div>
{:else}
  <!-- Plugin list view -->
  <div class="flex flex-col h-full">
    {#if !pluginsSupported}
      <div class="px-3 pt-2 shrink-0">
        <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-[11px] text-amber-700 dark:text-amber-300">
          {browserPluginSupportError ?? browserPluginSupport.reason ?? "Browser plugins are unavailable in this browser."}
        </div>
      </div>
    {/if}

    <!-- Plugin List -->
    <div class="flex-1 overflow-y-auto">
      {#if registryLoading}
        <div class="flex items-center justify-center py-8 text-muted-foreground gap-2">
          <Loader2 class="size-4 animate-spin" />
          <span class="text-xs">Loading...</span>
        </div>
      {:else if registryError}
        <div class="px-3 py-4 text-xs text-muted-foreground">{registryError}</div>
      {:else if filteredPlugins.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No plugins match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each filteredPlugins as plugin}
            {@const installed = installedIds.has(plugin.id)}
            {@const installing = installingIds.has(plugin.id)}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-foreground/40"
              onclick={() => (detailPlugin = plugin)}
            >
              <div class="flex items-center justify-between gap-2">
                <h3 class="text-xs font-medium truncate">{plugin.name}</h3>
                <Badge variant="secondary" class="text-[9px] shrink-0">v{plugin.version}</Badge>
              </div>
              <p class="text-[11px] text-muted-foreground mt-0.5 line-clamp-2">{plugin.summary}</p>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                {#if installed}
                  <span class="text-[11px] text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1">
                    <Check class="size-2.5" />Installed
                  </span>
                {:else}
                  <span class="text-[11px] text-muted-foreground">{plugin.author}</span>
                {/if}
                <Button
                  variant="outline"
                  size="sm"
                  class="h-6 text-[11px] px-2"
                  onclick={(e) => { e.stopPropagation(); void installFromRegistry(plugin); }}
                  disabled={installing || installed || !pluginsSupported}
                >
                  {#if installing}
                    <Loader2 class="size-3 mr-1 animate-spin" />
                  {:else}
                    <Download class="size-3 mr-1" />
                  {/if}
                  {installing ? "..." : "Install"}
                </Button>
              </div>
            </button>
          {/each}
        </div>
      {/if}

      <!-- Local Plugins -->
      {#if localPlugins.length > 0}
        <Separator />
        <div class="p-2 space-y-1.5">
          <h4 class="text-[11px] font-medium text-muted-foreground px-1">Local Plugins</h4>
          {#each localPlugins as plugin}
            {@const pluginId = String(plugin.id)}
            {@const removing = removingIds.has(pluginId)}
            <div class="rounded-md border p-2.5 flex items-center justify-between gap-2">
              <div class="min-w-0">
                <p class="text-xs font-medium truncate">{plugin.name}</p>
                <p class="text-[11px] text-muted-foreground truncate">{plugin.description || pluginId}</p>
              </div>
              <div class="flex items-center gap-1.5 shrink-0">
                <Switch
                  id={`mp-local-enabled-${pluginId}`}
                  checked={pluginStore.isPluginEnabled(pluginId)}
                  onCheckedChange={(checked) => setEnabled(pluginId, checked)}
                  disabled={!pluginsSupported}
                />
                <Button variant="ghost" size="icon-sm" onclick={() => removePlugin(pluginId, String(plugin.name ?? pluginId))} disabled={removing}>
                  {#if removing}
                    <Loader2 class="size-3 animate-spin" />
                  {:else}
                    <Trash2 class="size-3 text-destructive" />
                  {/if}
                </Button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Search + Filters + Add Local -->
    <div class="px-3 py-2 space-y-2 border-t shrink-0">
      {#if filtersOpen}
        <div class="space-y-1.5">
          <select class="w-full h-7 rounded-md border bg-background px-2 text-xs" bind:value={categoryFilter}>
            {#each categories as category}
              <option value={category}>{category === "all" ? "All categories" : category}</option>
            {/each}
          </select>
          <select class="w-full h-7 rounded-md border bg-background px-2 text-xs" bind:value={capabilityFilter}>
            {#each capabilities as capability}
              <option value={capability}>{capability === "all" ? "All capabilities" : capability}</option>
            {/each}
          </select>
          <div class="flex gap-1.5">
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={sourceFilter}>
              <option value="all">All</option>
              <option value="installed">Installed</option>
            </select>
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={sortBy}>
              <option value="name">Name</option>
              <option value="recent">Recent</option>
              <option value="version">Version</option>
            </select>
          </div>
        </div>
      {/if}
      <div class="flex items-center gap-2">
        <div class="relative flex-1 min-w-0">
          <Search class="size-3.5 absolute left-2 top-2 text-muted-foreground" />
          <Input class="pl-7 h-7 text-xs" placeholder="Search plugins" bind:value={search} />
        </div>
        <Button variant="outline" size="icon" class="size-7 shrink-0 {filtersOpen ? 'border-primary' : ''}" onclick={() => (filtersOpen = !filtersOpen)} aria-label="Toggle filters">
          <SlidersHorizontal class="size-3.5" />
        </Button>
        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={triggerUpload} disabled={uploadingLocal || !pluginsSupported} aria-label="Add local plugin">
          {#if uploadingLocal}
            <Loader2 class="size-3.5 animate-spin" />
          {:else}
            <Upload class="size-3.5" />
          {/if}
        </Button>
      </div>
    </div>
  </div>
{/if}
