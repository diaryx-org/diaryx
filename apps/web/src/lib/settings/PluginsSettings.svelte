<script lang="ts">
  import { Puzzle, Loader2, Trash2, Upload, Download, Check } from "@lucide/svelte";
  import { Switch } from "$lib/components/ui/switch";
  import { Button } from "$lib/components/ui/button";
  import { Separator } from "$lib/components/ui/separator";
  import { toast } from "svelte-sonner";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import {
    getBrowserPluginSupport,
    getBrowserPluginSupportError,
    installPlugin as browserInstallPlugin,
    uninstallPlugin as browserUninstallPlugin,
    isBuiltinPlugin,
  } from "$lib/plugins/browserPluginManager.svelte";
  import {
    fetchPluginRegistry,
    type RegistryPlugin,
  } from "$lib/plugins/pluginRegistry";
  import { isSyncPluginId } from "$lib/sync/syncBuiltinUiRegistry";
  import { getBackend, isTauri } from "$lib/backend";
  import type { Backend } from "$lib/backend/interface";

  const pluginStore = getPluginStore();

  // =========================================================================
  // State
  // =========================================================================

  let registryPlugins = $state<RegistryPlugin[]>([]);
  let registryLoading = $state(true);
  let registryError = $state<string | null>(null);
  let installingIds = $state<Set<string>>(new Set());
  let removingIds = $state<Set<string>>(new Set());
  let uploadingCustom = $state(false);
  let fileInputRef = $state<HTMLInputElement | null>(null);

  // =========================================================================
  // Derived
  // =========================================================================

  const browserPluginSupport = $derived(getBrowserPluginSupport());
  const browserPluginSupportError = $derived(getBrowserPluginSupportError());

  /** All installed plugin IDs (from all sources). */
  const installedIds = $derived(
    new Set(pluginStore.allManifests.map((m) => String(m.id))),
  );

  /** Custom (user-uploaded) plugins: installed but not in the registry and not sync. */
  const customPlugins = $derived.by(() => {
    const registryIds = new Set(registryPlugins.map((r) => r.id));
    return pluginStore.allManifests
      .filter((m) => {
        const id = String(m.id);
        return !registryIds.has(id) && !isSyncPluginId(id) && !isBuiltinPlugin(id);
      })
      .sort((a, b) => {
        const aName = String(a.name ?? a.id);
        const bName = String(b.name ?? b.id);
        return aName.localeCompare(bName);
      });
  });

  // =========================================================================
  // Registry Fetch
  // =========================================================================

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

  // Fetch on mount
  $effect(() => {
    loadRegistry();
  });

  // =========================================================================
  // Platform-aware install/uninstall
  // =========================================================================

  async function platformInstall(
    wasmBytes: ArrayBuffer,
    name?: string,
  ): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.installPlugin) {
        await backend.installPlugin(new Uint8Array(wasmBytes));
        return;
      }
    }
    await browserInstallPlugin(wasmBytes, name);
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

  // =========================================================================
  // Actions
  // =========================================================================

  async function installFromRegistry(rp: RegistryPlugin) {
    installingIds = new Set([...installingIds, rp.id]);
    try {
      const resp = await fetch(rp.wasmUrl);
      if (!resp.ok) throw new Error(`Download failed: ${resp.status}`);
      const bytes = await resp.arrayBuffer();
      await platformInstall(bytes, rp.name);
      toast.success(`Installed ${rp.name}`);
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : `Failed to install ${rp.name}`,
      );
    } finally {
      installingIds = new Set([...installingIds].filter((id) => id !== rp.id));
    }
  }

  async function removePlugin(pluginId: string, pluginName: string) {
    removingIds = new Set([...removingIds, pluginId]);
    try {
      await platformUninstall(pluginId);
      toast.success(`Removed ${pluginName}`);
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : `Failed to remove ${pluginName}`,
      );
    } finally {
      removingIds = new Set([...removingIds].filter((id) => id !== pluginId));
    }
  }

  function triggerFileUpload() {
    fileInputRef?.click();
  }

  async function handleFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";

    if (!file.name.endsWith(".wasm")) {
      toast.error("Please select a .wasm file");
      return;
    }

    uploadingCustom = true;
    try {
      const bytes = await file.arrayBuffer();
      await platformInstall(bytes, file.name.replace(/\.wasm$/, ""));
      toast.success(`Installed plugin from ${file.name}`);
    } catch (e) {
      toast.error(
        e instanceof Error ? e.message : "Failed to install plugin",
      );
    } finally {
      uploadingCustom = false;
    }
  }

  function isEnabled(pluginId: string): boolean {
    return pluginStore.isPluginEnabled(pluginId);
  }

  function setEnabled(pluginId: string, enabled: boolean): void {
    pluginStore.setPluginEnabled(pluginId, enabled);
  }
</script>

<div class="space-y-5">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <h3 class="font-medium flex items-center gap-2">
      <Puzzle class="size-4" />
      Plugins
    </h3>
    <Button
      variant="outline"
      size="sm"
      onclick={triggerFileUpload}
      disabled={uploadingCustom || !browserPluginSupport.supported}
    >
      {#if uploadingCustom}
        <Loader2 class="size-3.5 mr-1.5 animate-spin" />
        Installing...
      {:else}
        <Upload class="size-3.5 mr-1.5" />
        Add Plugin
      {/if}
    </Button>
  </div>

  <!-- Hidden file input for custom .wasm upload -->
  <input
    type="file"
    accept=".wasm"
    class="hidden"
    bind:this={fileInputRef}
    onchange={handleFileSelected}
  />

  <!-- Browser support warning -->
  {#if !browserPluginSupport.supported}
    <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2">
      <p class="text-xs text-amber-700 dark:text-amber-300">
        {browserPluginSupportError ?? browserPluginSupport.reason ?? 'Browser plugins are unavailable in this browser.'}
      </p>
    </div>
  {/if}

  <!-- Available Plugins (from registry) -->
  <div class="space-y-2">
    <h4 class="text-sm font-medium text-muted-foreground">Available Plugins</h4>

    {#if registryLoading}
      <div class="flex items-center gap-2 px-1 py-3">
        <Loader2 class="size-3.5 animate-spin text-muted-foreground" />
        <span class="text-sm text-muted-foreground">Loading plugin registry...</span>
      </div>
    {:else if registryError}
      <p class="text-xs text-muted-foreground px-1">
        Could not load plugin registry. Installed plugins still work.
      </p>
    {:else if registryPlugins.length === 0}
      <p class="text-sm text-muted-foreground px-1">No plugins available.</p>
    {:else}
      <div class="space-y-2">
        {#each registryPlugins as rp}
          {@const installed = installedIds.has(rp.id)}
          {@const installing = installingIds.has(rp.id)}
          <div class="flex items-center justify-between gap-3 rounded-md border p-3">
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium">{rp.name}</span>
                {#if rp.builtin}
                  <span class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-muted text-muted-foreground">Built-in</span>
                {/if}
                <span class="text-[10px] text-muted-foreground">v{rp.version}</span>
              </div>
              <p class="text-xs text-muted-foreground mt-0.5 truncate">{rp.description}</p>
            </div>
            <div class="flex items-center gap-2 shrink-0">
              {#if installed}
                <Switch
                  id={`plugin-enabled-${rp.id}`}
                  checked={isEnabled(rp.id)}
                  onCheckedChange={(checked) => setEnabled(rp.id, checked)}
                  disabled={!browserPluginSupport.supported}
                />
                <span class="text-xs text-emerald-600 dark:text-emerald-400 flex items-center gap-1">
                  <Check class="size-3" />
                  Installed
                </span>
              {:else}
                <Button
                  variant="outline"
                  size="sm"
                  onclick={() => installFromRegistry(rp)}
                  disabled={installing || !browserPluginSupport.supported}
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
        {/each}
      </div>
    {/if}
  </div>

  <!-- Custom Plugins (user-uploaded) -->
  {#if customPlugins.length > 0 || (!registryLoading && !registryError)}
    <Separator />

    <div class="space-y-2">
      <h4 class="text-sm font-medium text-muted-foreground">Custom Plugins</h4>

      {#if customPlugins.length === 0}
        <p class="text-xs text-muted-foreground px-1">
          No custom plugins. Upload a .wasm file to add one.
        </p>
      {:else}
        <div class="space-y-2">
          {#each customPlugins as plugin}
            {@const pluginId = String(plugin.id)}
            {@const removing = removingIds.has(pluginId)}
            <div class="flex items-center justify-between gap-3 rounded-md border p-3">
              <div class="flex-1 min-w-0">
                <span class="text-sm font-medium">{plugin.name}</span>
                <p class="text-xs text-muted-foreground mt-0.5 truncate">
                  {plugin.description || pluginId}
                </p>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                <Switch
                  id={`plugin-enabled-${pluginId}`}
                  checked={isEnabled(pluginId)}
                  onCheckedChange={(checked) => setEnabled(pluginId, checked)}
                  disabled={!browserPluginSupport.supported}
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
  {/if}
</div>
