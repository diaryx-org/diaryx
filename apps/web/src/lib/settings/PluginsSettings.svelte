<script lang="ts">
  import {
    Puzzle,
    Loader2,
    Trash2,
    Upload,
    Download,
    Check,
    Store,
  } from "@lucide/svelte";
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
    inspectPluginWasm,
  } from "$lib/plugins/browserPluginManager.svelte";
  import {
    fetchPluginRegistry,
    type RegistryPlugin,
  } from "$lib/plugins/pluginRegistry";
  import { getBackend, isTauri } from "$lib/backend";
  import { createApi } from "$lib/backend/api";
  import type { Backend } from "$lib/backend/interface";
  import { workspaceStore } from "@/models/stores/workspaceStore.svelte";
  import type {
    PermissionType,
    PluginConfig,
    PluginPermissions,
  } from "@/models/stores/permissionStore.svelte";

  interface Props {
    onOpenMarketplace?: () => void;
  }

  let { onOpenMarketplace }: Props = $props();

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
  // In Tauri, plugins load natively — browser WASM support is irrelevant
  const pluginsSupported = $derived(isTauri() || browserPluginSupport.supported);

  /** All installed plugin IDs (from all sources). */
  const installedIds = $derived(
    new Set(pluginStore.allManifests.map((m) => String(m.id))),
  );

  /** Local (user-uploaded) plugins: installed but not in the curated registry. */
  const localPlugins = $derived.by(() => {
    const registryIds = new Set(registryPlugins.map((r) => r.id));
    return pluginStore.allManifests
      .filter((m) => {
        const id = String(m.id);
        return !registryIds.has(id);
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
    expectedPluginId?: string,
  ): Promise<void> {
    if (isTauri()) {
      const backend: Backend = await getBackend();
      if (backend.installPlugin) {
        const manifestJson = await backend.installPlugin(new Uint8Array(wasmBytes));
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

  async function verifyRegistryArtifact(
    wasmBytes: ArrayBuffer,
    expectedSha256: string,
  ): Promise<void> {
    const actual = await sha256Hex(wasmBytes);
    if (actual !== normalizeSha256(expectedSha256)) {
      throw new Error("Plugin integrity check failed (SHA-256 mismatch)");
    }
  }

  async function persistDefaultPermissions(
    pluginId: string,
    defaults: PluginPermissions,
  ): Promise<void> {
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

    await api.setFrontmatterProperty(
      rootIndexPath,
      "plugins",
      nextPlugins as any,
      rootIndexPath,
    );
  }

  async function reviewAndInstall(
    wasmBytes: ArrayBuffer,
    fallbackName?: string,
    expectedPluginId?: string,
  ): Promise<void> {
    // On Tauri, the native backend handles WASM loading/inspection — browser
    // Extism isn't available on iOS. Skip browser-side inspect and install
    // directly; the backend extracts the manifest and returns it.
    if (isTauri()) {
      await platformInstall(wasmBytes, fallbackName, expectedPluginId);
      return;
    }

    const inspected = await inspectPluginWasm(wasmBytes);
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

    await platformInstall(wasmBytes, fallbackName ?? pluginName, expectedPluginId);
  }

  async function installFromRegistry(rp: RegistryPlugin) {
    installingIds = new Set([...installingIds, rp.id]);
    try {
      const resp = await fetch(rp.artifact.wasmUrl);
      if (!resp.ok) throw new Error(`Download failed: ${resp.status}`);
      const bytes = await resp.arrayBuffer();
      await verifyRegistryArtifact(bytes, rp.artifact.sha256);
      await reviewAndInstall(bytes, rp.name, rp.id);
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
      await reviewAndInstall(bytes, file.name.replace(/\.wasm$/, ""));
      toast.success(`Installed local plugin from ${file.name}`);
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
  <div class="flex items-center justify-between gap-2">
    <h3 class="font-medium flex items-center gap-2">
      <Puzzle class="size-4" />
      Plugins
    </h3>
    <div class="flex items-center gap-2">
      {#if onOpenMarketplace}
        <Button variant="outline" size="sm" onclick={onOpenMarketplace}>
          <Store class="size-3.5 mr-1.5" />
          Open Marketplace
        </Button>
      {/if}
      <Button
        variant="outline"
        size="sm"
        onclick={triggerFileUpload}
        disabled={uploadingCustom || !pluginsSupported}
      >
        {#if uploadingCustom}
          <Loader2 class="size-3.5 mr-1.5 animate-spin" />
          Installing...
        {:else}
          <Upload class="size-3.5 mr-1.5" />
          Add Local Plugin
        {/if}
      </Button>
    </div>
  </div>

  <!-- Hidden file input for local .wasm upload -->
  <input
    type="file"
    accept=".wasm"
    class="hidden"
    bind:this={fileInputRef}
    onchange={handleFileSelected}
  />

  <!-- Browser support warning -->
  {#if !pluginsSupported}
    <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2">
      <p class="text-xs text-amber-700 dark:text-amber-300">
        {browserPluginSupportError ?? browserPluginSupport.reason ?? 'Browser plugins are unavailable in this browser.'}
      </p>
    </div>
  {/if}

  <!-- Curated Plugins (from registry-v2) -->
  <div class="space-y-2">
    <h4 class="text-sm font-medium text-muted-foreground">Curated Plugins</h4>

    {#if registryLoading}
      <div class="flex items-center gap-2 px-1 py-3">
        <Loader2 class="size-3.5 animate-spin text-muted-foreground" />
        <span class="text-sm text-muted-foreground">Loading plugin registry...</span>
      </div>
    {:else if registryError}
      <p class="text-xs text-muted-foreground px-1">
        Could not load plugin registry v2. Installed plugins still work.
      </p>
    {:else if registryPlugins.length === 0}
      <p class="text-sm text-muted-foreground px-1">No curated plugins available.</p>
    {:else}
      <div class="space-y-2">
        {#each registryPlugins as rp}
          {@const installed = installedIds.has(rp.id)}
          {@const installing = installingIds.has(rp.id)}
          <div class="flex items-center justify-between gap-3 rounded-md border p-3">
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="text-sm font-medium">{rp.name}</span>
                <span class="text-[10px] text-muted-foreground">v{rp.version}</span>
                <span class="text-[10px] rounded bg-muted px-1.5 py-0.5 text-muted-foreground">
                  {rp.source.kind}
                </span>
              </div>
              <p class="text-xs text-muted-foreground mt-0.5 line-clamp-2">{rp.summary}</p>
              <p class="text-[11px] text-muted-foreground mt-1">
                {rp.creator} • {rp.license}
              </p>
            </div>
            <div class="flex items-center gap-2 shrink-0">
              {#if installed}
                <Switch
                  id={`plugin-enabled-${rp.id}`}
                  checked={isEnabled(rp.id)}
                  onCheckedChange={(checked) => setEnabled(rp.id, checked)}
                  disabled={!pluginsSupported}
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
        {/each}
      </div>
    {/if}
  </div>

  <!-- Local plugins -->
  {#if localPlugins.length > 0 || (!registryLoading && !registryError)}
    <Separator />

    <div class="space-y-2">
      <h4 class="text-sm font-medium text-muted-foreground">Local Plugins (Unmanaged)</h4>

      {#if localPlugins.length === 0}
        <p class="text-xs text-muted-foreground px-1">
          No local plugins. Upload a .wasm file to add one.
        </p>
      {:else}
        <div class="space-y-2">
          {#each localPlugins as plugin}
            {@const pluginId = String(plugin.id)}
            {@const removing = removingIds.has(pluginId)}
            <div class="flex items-center justify-between gap-3 rounded-md border p-3">
              <div class="flex-1 min-w-0">
                <span class="text-sm font-medium">{plugin.name}</span>
                <p class="text-xs text-muted-foreground mt-0.5 truncate">
                  {plugin.description || pluginId}
                </p>
                <p class="text-[11px] text-muted-foreground mt-1">Source: local upload</p>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                <Switch
                  id={`plugin-enabled-${pluginId}`}
                  checked={isEnabled(pluginId)}
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
  {/if}
</div>
