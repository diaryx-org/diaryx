<script lang="ts">
  import {
    ArrowLeft,
    Check,
    ExternalLink,
    Loader2,
    Search,
    SlidersHorizontal,
    Sparkles,
    Upload,
    WandSparkles,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  import * as Dialog from "$lib/components/ui/dialog";
  import { openExternalUrl } from "$lib/billing";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    fetchBundleRegistry,
    normalizeBundleRegistryEntry,
    type BundleRegistry,
  } from "$lib/marketplace/bundleRegistry";
  import {
    executeBundleApply,
    planBundleApply,
    type BundleApplyPlan,
    type BundleApplyResult,
  } from "$lib/marketplace/bundleApply";
  import {
    fetchThemeRegistry,
    type ThemeRegistry,
  } from "$lib/marketplace/themeRegistry";
  import {
    fetchTypographyRegistry,
    type TypographyRegistry,
  } from "$lib/marketplace/typographyRegistry";
  import type {
    BundleRegistryEntry,
    TypographyRegistryEntry,
  } from "$lib/marketplace/types";
  import {
    fetchPluginRegistry,
    type RegistryPlugin,
  } from "$lib/plugins/pluginRegistry";
  import { useMarketplacePanel, isRecord } from "./useMarketplacePanel.svelte";

  interface DisplayBundle extends BundleRegistryEntry {
    source: "registry" | "local";
  }

  // Side-loaded registries for the detail view
  let registryThemes = $state<ThemeRegistry["themes"]>([]);
  let registryTypographies = $state<TypographyRegistry["typographies"]>([]);
  let registryPlugins = $state<RegistryPlugin[]>([]);

  const panel = useMarketplacePanel<DisplayBundle>({
    async fetchItems() {
      const [bundleRegistry, themeRegistry, typographyRegistry, pluginRegistry] =
        await Promise.all([
          fetchBundleRegistry(),
          fetchThemeRegistry(),
          fetchTypographyRegistry(),
          fetchPluginRegistry(),
        ]);

      // Populate side-loaded registries
      registryThemes = (themeRegistry as ThemeRegistry).themes;
      registryTypographies = (typographyRegistry as TypographyRegistry).typographies;
      registryPlugins = pluginRegistry.plugins;

      return (bundleRegistry as BundleRegistry).bundles.map(
        (entry) => ({ ...entry, source: "registry" as const }),
      );
    },
    getTimestamp(item) {
      return item.artifact?.published_at ? Date.parse(item.artifact.published_at) || 0 : 0;
    },
    sourceFilterOptions: [
      ["all", "All sources"],
      ["curated", "Curated"],
      ["local", "Local"],
    ],
    matchesSourceFilter(item, filter) {
      if (filter === "curated") return item.source === "registry";
      if (filter === "local") return item.source === "local";
      return true;
    },
    parseLocalPayload(payload) {
      if (!payload) throw new Error("Bundle file is empty");

      if (isRecord(payload) && Array.isArray(payload.bundles)) {
        return payload.bundles.map(
          (entry) => ({ ...normalizeBundleRegistryEntry(entry), source: "local" as const }),
        );
      }
      if (isRecord(payload) && "bundle" in payload) {
        return [{ ...normalizeBundleRegistryEntry(payload.bundle), source: "local" as const }];
      }
      return [{ ...normalizeBundleRegistryEntry(payload), source: "local" as const }];
    },
  });

  // Confirmation dialog state
  let confirmDialogOpen = $state(false);
  let pendingPlan = $state<BundleApplyPlan | null>(null);
  let lastApplyResult = $state<BundleApplyResult | null>(null);

  const typographyById = $derived.by(() => {
    return new Map(
      registryTypographies.map((typography) => [typography.id, typography]),
    );
  });

  const themeById = $derived.by(() => {
    return new Map(registryThemes.map((theme) => [theme.id, theme]));
  });

  function bundleThemeSwatches(themeId: string, mode: "light" | "dark"): string[] {
    const entry = themeById.get(themeId);
    if (!entry) return [];
    const palette = mode === "dark" ? entry.theme.colors.dark : entry.theme.colors.light;
    return [palette.background, palette.primary, palette.accent, palette.muted, palette.foreground];
  }

  function resolveTypographyLabel(
    typographyId: string | null,
  ): string {
    if (!typographyId) return "None";
    const typography = typographyById.get(typographyId) as TypographyRegistryEntry | undefined;
    return typography ? `${typography.name} (${typographyId})` : typographyId;
  }

  function openApplyConfirmation(bundle: DisplayBundle): void {
    pendingPlan = planBundleApply(bundle, {
      themes: registryThemes,
      typographies: registryTypographies,
      plugins: registryPlugins,
    });
    lastApplyResult = null;
    confirmDialogOpen = true;
  }

  async function applyPendingBundle(): Promise<void> {
    if (!pendingPlan) return;

    const bundleId = pendingPlan.bundle.id;
    panel.addActiveId(bundleId);

    try {
      const result = await executeBundleApply(pendingPlan);
      lastApplyResult = result;

      if (result.summary.failed === 0) {
        toast.success(`Applied bundle ${pendingPlan.bundle.name}`);
      } else {
        toast.warning(
          `Applied bundle ${pendingPlan.bundle.name} with ${result.summary.failed} failed action${
            result.summary.failed === 1 ? "" : "s"
          }`,
        );
      }
    } finally {
      panel.removeActiveId(bundleId);
    }
  }

  async function handleLocalFileSelected(event: Event): Promise<void> {
    try {
      const items = await panel.onLocalFileSelected(event);
      if (items.length > 0) {
        toast.success(`Imported ${items.length} bundle${items.length === 1 ? "" : "s"}`);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Invalid bundle file");
    }
  }
</script>

<input
  type="file"
  accept=".json"
  class="hidden"
  bind:this={panel.localFileInputRef}
  onchange={handleLocalFileSelected}
/>

<Dialog.Root bind:open={confirmDialogOpen}>
  <Dialog.Content class="sm:max-w-[520px]">
    <Dialog.Header>
      <Dialog.Title>Apply Bundle</Dialog.Title>
      <Dialog.Description>
        Review planned actions before applying this bundle.
      </Dialog.Description>
    </Dialog.Header>

    {#if pendingPlan}
      <div class="space-y-3 text-sm max-h-[360px] overflow-y-auto pr-1">
        <div class="rounded-md border p-2 text-xs">
          <p class="font-medium">{pendingPlan.bundle.name}</p>
          <p class="text-muted-foreground mt-1">{pendingPlan.bundle.summary}</p>
        </div>

        <div class="space-y-1">
          <p class="text-xs font-medium">Actions</p>
          <ul class="space-y-1">
            {#each pendingPlan.actions as action}
              <li class="text-xs rounded-md border px-2 py-1.5">
                {action.label}
                {#if !action.required}
                  <span class="text-muted-foreground"> (optional)</span>
                {/if}
              </li>
            {/each}
          </ul>
        </div>

        {#if pendingPlan.missingTheme}
          <p class="text-xs text-destructive">
            Theme "{pendingPlan.bundle.theme_id}" was not found in the curated theme registry.
          </p>
        {/if}

        {#if pendingPlan.missingTypographyPreset && pendingPlan.bundle.typography_id}
          <p class="text-xs text-muted-foreground">
            Typography "{pendingPlan.bundle.typography_id}" was not found in the curated typography registry.
          </p>
        {/if}

        {#if pendingPlan.missingRequiredPlugins.length > 0}
          <p class="text-xs text-destructive">
            Missing required plugins: {pendingPlan.missingRequiredPlugins.join(", ")}
          </p>
        {/if}

        {#if pendingPlan.missingOptionalPlugins.length > 0}
          <p class="text-xs text-muted-foreground">
            Missing optional plugins: {pendingPlan.missingOptionalPlugins.join(", ")}
          </p>
        {/if}

        {#if lastApplyResult}
          <div class="space-y-1">
            <p class="text-xs font-medium">Latest result</p>
            <p class="text-xs text-muted-foreground">
              {lastApplyResult.summary.success} succeeded, {lastApplyResult.summary.failed} failed.
            </p>
          </div>
        {/if}
      </div>

      <Dialog.Footer>
        <Button variant="outline" onclick={() => (confirmDialogOpen = false)}>
          Close
        </Button>
        <Button
          onclick={() => void applyPendingBundle()}
          disabled={panel.isActive(pendingPlan.bundle.id)}
        >
          {#if panel.isActive(pendingPlan.bundle.id)}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />Applying...
          {:else}
            <WandSparkles class="size-3.5 mr-1.5" />Apply Bundle
          {/if}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>

{#if panel.detailItem}
  {@const bundle = panel.detailItem}
  {@const applying = panel.isActive(bundle.id)}
  <div class="flex flex-col h-full">
    <div class="flex items-center gap-2 px-3 py-2 border-b shrink-0">
      <Button variant="ghost" size="icon" class="size-7" onclick={() => (panel.detailId = null)} aria-label="Back">
        <ArrowLeft class="size-4" />
      </Button>
      <h3 class="text-sm font-medium truncate">{bundle.name}</h3>
    </div>

    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
      <div class="space-y-1">
        <div class="flex items-center gap-2 flex-wrap">
          <Badge variant="secondary">v{bundle.version}</Badge>
          <Badge variant="outline">{bundle.source === "registry" ? "Curated" : "Local"}</Badge>
        </div>
        <p class="text-xs text-muted-foreground">{bundle.summary}</p>
        <p class="text-xs">{bundle.description}</p>
      </div>

      <div class="grid grid-cols-2 gap-1.5 text-[11px]">
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Theme</p>
          <p class="font-medium">{bundle.theme_id}</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Plugins</p>
          <p class="font-medium">{bundle.plugins.length}</p>
        </div>
      </div>

      {#if bundle.typography_id || bundle.typography}
        <div class="rounded-md border p-2 text-[11px] space-y-1">
          <p class="font-medium">Typography preset</p>
          <p>{resolveTypographyLabel(bundle.typography_id)}</p>
          {#if bundle.typography?.fontFamily}
            <p>Override font: {bundle.typography.fontFamily}</p>
          {/if}
          {#if typeof bundle.typography?.baseFontSize === "number"}
            <p>Override size: {bundle.typography.baseFontSize}px</p>
          {/if}
          {#if typeof bundle.typography?.lineHeight === "number"}
            <p>Override line height: {bundle.typography.lineHeight}</p>
          {/if}
          {#if bundle.typography?.contentWidth}
            <p>Override width: {bundle.typography.contentWidth}</p>
          {/if}
        </div>
      {/if}

      {#if bundle.plugins.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Plugin dependencies</h4>
          <div class="space-y-1">
            {#each bundle.plugins as dependency}
              <div class="rounded-md border p-1.5 text-[11px]">
                <p class="font-medium">{dependency.plugin_id}</p>
                <p class="text-muted-foreground">
                  {dependency.required ? "Required" : "Optional"}
                  {dependency.enable ? " • Enable on apply" : ""}
                </p>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if bundle.tags.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Tags</h4>
          <div class="flex flex-wrap gap-1">
            {#each bundle.tags as tag}
              <Badge variant="secondary" class="text-[10px]">{tag}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if bundle.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          onclick={() => openExternalUrl(bundle.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}

      <div class="flex items-center gap-2 pt-1">
        <Button size="sm" onclick={() => openApplyConfirmation(bundle)} disabled={applying}>
          {#if applying}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />Applying...
          {:else}
            <Sparkles class="size-3.5 mr-1.5" />Apply Bundle
          {/if}
        </Button>
      </div>
    </div>
  </div>
{:else}
  <div class="flex flex-col h-full">
    <div class="flex-1 overflow-y-auto">
      {#if panel.loadError}
        <div class="px-3 pt-2">
          <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-[11px] text-amber-700 dark:text-amber-300">
            {panel.loadError}
          </div>
        </div>
      {/if}

      {#if panel.loading}
        <div class="flex items-center justify-center py-8 text-muted-foreground gap-2">
          <Loader2 class="size-4 animate-spin" />
          <span class="text-xs">Loading bundles...</span>
        </div>
      {:else if panel.filteredItems.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No bundles match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each panel.filteredItems as bundle}
            {@const applying = panel.isActive(bundle.id)}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-muted-foreground"
              onclick={() => (panel.detailId = bundle.id)}
            >
              {#if themeById.has(bundle.theme_id)}
                <div class="flex gap-1.5 mb-1.5">
                  <div class="flex items-center gap-px">
                    {#each bundleThemeSwatches(bundle.theme_id, "light") as swatch}
                      <span class="size-3 first:rounded-l-sm last:rounded-r-sm border border-black/10" style:background={swatch}></span>
                    {/each}
                  </div>
                  <div class="flex items-center gap-px">
                    {#each bundleThemeSwatches(bundle.theme_id, "dark") as swatch}
                      <span class="size-3 first:rounded-l-sm last:rounded-r-sm border border-white/20" style:background={swatch}></span>
                    {/each}
                  </div>
                  <span class="text-[9px] text-muted-foreground ml-auto self-center">
                    {bundle.plugins.length} plugin{bundle.plugins.length === 1 ? "" : "s"}
                  </span>
                </div>
              {/if}
              <div class="flex items-center justify-between gap-2">
                <h3 class="text-xs font-medium truncate">{bundle.name}</h3>
                <Badge variant="secondary" class="text-[9px] shrink-0">v{bundle.version}</Badge>
              </div>
              <p class="text-[11px] text-muted-foreground mt-0.5 line-clamp-1">{bundle.summary}</p>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[11px] text-muted-foreground">
                  {bundle.source === "registry" ? "Curated" : "Local"}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  class="h-6 text-[11px] px-2"
                  onclick={(event) => {
                    event.stopPropagation();
                    openApplyConfirmation(bundle);
                  }}
                  disabled={applying}
                >
                  {#if applying}
                    <Loader2 class="size-3 mr-1 animate-spin" />
                  {:else}
                    <Check class="size-3 mr-1" />
                  {/if}
                  Apply
                </Button>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="px-3 py-2 space-y-2 border-t shrink-0">
      {#if panel.filtersOpen}
        <div class="space-y-1.5">
          <select class="w-full h-7 rounded-md border bg-background px-2 text-xs" bind:value={panel.categoryFilter}>
            {#each panel.categories as category}
              <option value={category}>{category === "all" ? "All categories" : category}</option>
            {/each}
          </select>
          <div class="flex gap-1.5">
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={panel.sourceFilter}>
              <option value="all">All sources</option>
              <option value="curated">Curated</option>
              <option value="local">Local</option>
            </select>
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={panel.sortBy}>
              <option value="name">Name</option>
              <option value="recent">Recent</option>
            </select>
          </div>
        </div>
      {/if}

      <div class="flex items-center gap-2">
        <div class="relative flex-1 min-w-0">
          <Search class="size-3.5 absolute left-2 top-2 text-muted-foreground" />
          <Input class="pl-7 h-7 text-xs" placeholder="Search bundles" bind:value={panel.search} />
        </div>

        <Button
          variant="outline"
          size="icon"
          class="size-7 shrink-0 {panel.filtersOpen ? 'border-primary' : ''}"
          onclick={() => (panel.filtersOpen = !panel.filtersOpen)}
          aria-label="Toggle filters"
        >
          <SlidersHorizontal class="size-3.5" />
        </Button>

        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={panel.triggerLocalImport} aria-label="Import local bundle">
          {#if panel.importingLocal}
            <Loader2 class="size-3.5 animate-spin" />
          {:else}
            <Upload class="size-3.5" />
          {/if}
        </Button>
      </div>
    </div>
  </div>
{/if}
