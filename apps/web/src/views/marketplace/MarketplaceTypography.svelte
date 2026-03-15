<script lang="ts">
  import {
    ArrowLeft,
    Check,
    Download,
    ExternalLink,
    Loader2,
    Search,
    SlidersHorizontal,
    Trash2,
    Upload,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  import { openExternalUrl } from "$lib/billing";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    fetchTypographyRegistry,
    type TypographyRegistry,
  } from "$lib/marketplace/typographyRegistry";
  import type { TypographyRegistryEntry } from "$lib/marketplace/types";
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import {
    FONT_FAMILY_MAP,
    type ContentWidth,
    type FontFamily,
    type TypographyDefinition,
  } from "$lib/stores/appearance.types";

  type SourceFilter = "all" | "installed" | "available";
  type SortBy = "name" | "recent" | "installed";

  interface TypographyListEntry {
    id: string;
    typography: TypographyDefinition;
    name: string;
    version: string;
    summary: string;
    description: string;
    author: string;
    license: string;
    repository: string | null;
    categories: string[];
    tags: string[];
    styles: string[];
    screenshots: string[];
    installed: boolean;
    builtin: boolean;
    active: boolean;
    registry: TypographyRegistryEntry | null;
    installedAt: number | null;
    publishedAt: number | null;
  }

  const appearanceStore = getAppearanceStore();

  const fontOptions: { value: FontFamily; label: string }[] = [
    { value: "inter", label: "Inter" },
    { value: "system", label: "System" },
    { value: "serif", label: "Serif" },
    { value: "mono", label: "Mono" },
  ];

  const fontSizeOptions = [14, 15, 16, 17, 18, 19, 20, 22];

  const lineHeightOptions: { value: number; label: string }[] = [
    { value: 1.4, label: "Compact" },
    { value: 1.5, label: "Snug" },
    { value: 1.6, label: "Default" },
    { value: 1.8, label: "Relaxed" },
    { value: 2.0, label: "Loose" },
  ];

  const widthOptions: { value: ContentWidth; label: string }[] = [
    { value: "narrow", label: "Narrow" },
    { value: "medium", label: "Medium" },
    { value: "wide", label: "Wide" },
    { value: "full", label: "Full" },
  ];

  let registryTypographies = $state<TypographyRegistryEntry[]>([]);
  let registryLoading = $state(true);
  let registryError = $state<string | null>(null);

  let search = $state("");
  let filtersOpen = $state(false);
  let sourceFilter = $state<SourceFilter>("all");
  let categoryFilter = $state("all");
  let styleFilter = $state("all");
  let sortBy = $state<SortBy>("name");

  let importingLocal = $state(false);
  let detailTypographyId = $state<string | null>(null);
  let localFileInputRef = $state<HTMLInputElement | null>(null);

  function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
  }

  function isTypographyDefinition(value: unknown): value is TypographyDefinition {
    if (!isRecord(value)) return false;
    return (
      typeof value.id === "string" &&
      typeof value.name === "string" &&
      value.version === 1 &&
      isRecord(value.settings) &&
      typeof value.settings.baseFontSize === "number" &&
      typeof value.settings.lineHeight === "number"
    );
  }

  function extractImportedTypography(value: unknown): TypographyDefinition | null {
    if (
      isRecord(value) &&
      "typography" in value &&
      isTypographyDefinition(value.typography)
    ) {
      return value.typography;
    }

    if (isTypographyDefinition(value)) {
      return value;
    }

    return null;
  }

  async function loadRegistry(): Promise<void> {
    registryLoading = true;
    registryError = null;

    try {
      const registry: TypographyRegistry = await fetchTypographyRegistry();
      registryTypographies = registry.typographies;
    } catch (error) {
      registryTypographies = [];
      registryError =
        error instanceof Error ? error.message : "Failed to load typography registry";
    } finally {
      registryLoading = false;
    }
  }

  $effect(() => {
    loadRegistry();
  });

  const allEntries = $derived.by(() => {
    const installedTypographies = appearanceStore.listTypographies();
    const installedById = new Map(
      installedTypographies.map((entry) => [entry.typography.id, entry]),
    );
    const registryById = new Map(
      registryTypographies.map((entry) => [entry.id, entry]),
    );

    const ids = new Set<string>([
      ...Array.from(installedById.keys()),
      ...Array.from(registryById.keys()),
    ]);

    const entries: TypographyListEntry[] = [];

    for (const id of ids) {
      const installed = installedById.get(id);
      const registry = registryById.get(id) ?? null;
      const typography = installed?.typography ?? registry?.typography;
      if (!typography) continue;

      const installedAt =
        typeof installed?.source.installedAt === "number"
          ? installed.source.installedAt
          : null;
      const publishedAt = registry?.artifact?.published_at
        ? Date.parse(registry.artifact.published_at) || null
        : null;

      entries.push({
        id,
        typography,
        name: registry?.name ?? typography.name,
        version: registry?.version ?? `${typography.version}`,
        summary:
          registry?.summary ??
          typography.description ??
          "A custom Diaryx typography preset.",
        description:
          registry?.description ??
          typography.description ??
          "No description provided.",
        author: registry?.author ?? typography.author ?? "Unknown",
        license: registry?.license ?? "Custom",
        repository: registry?.repository ?? null,
        categories: registry?.categories ?? [],
        tags: registry?.tags ?? [],
        styles: registry?.styles ?? [],
        screenshots: registry?.screenshots ?? [],
        installed: !!installed,
        builtin: installed?.builtin ?? false,
        active: appearanceStore.typographyPresetId === id,
        registry,
        installedAt,
        publishedAt,
      });
    }

    return entries;
  });

  const categories = $derived.by(() => {
    const all = new Set<string>();
    for (const entry of allEntries) {
      for (const category of entry.categories) all.add(category);
    }
    return ["all", ...Array.from(all).sort()];
  });

  const styles = $derived.by(() => {
    const all = new Set<string>();
    for (const entry of allEntries) {
      for (const style of entry.styles) all.add(style);
    }
    return ["all", ...Array.from(all).sort()];
  });

  const filteredEntries = $derived.by(() => {
    const query = search.trim().toLowerCase();

    const filtered = allEntries.filter((entry) => {
      if (sourceFilter === "installed" && !entry.installed) return false;
      if (sourceFilter === "available" && !entry.registry) return false;
      if (categoryFilter !== "all" && !entry.categories.includes(categoryFilter)) return false;
      if (styleFilter !== "all" && !entry.styles.includes(styleFilter)) return false;

      if (!query) return true;
      const haystack = [
        entry.id,
        entry.name,
        entry.summary,
        entry.description,
        entry.author,
        ...entry.tags,
        ...entry.categories,
        ...entry.styles,
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(query);
    });

    filtered.sort((a, b) => {
      if (sortBy === "installed") {
        if (a.installed !== b.installed) {
          return Number(b.installed) - Number(a.installed);
        }
        return a.name.localeCompare(b.name);
      }
      if (sortBy === "name") {
        return a.name.localeCompare(b.name);
      }
      const aTs = a.publishedAt ?? a.installedAt ?? 0;
      const bTs = b.publishedAt ?? b.installedAt ?? 0;
      return bTs - aTs;
    });

    return filtered;
  });

  const detailEntry = $derived.by(() => {
    if (!detailTypographyId) return null;
    return allEntries.find((entry) => entry.id === detailTypographyId) ?? null;
  });

  async function installTypography(entry: TypographyListEntry): Promise<void> {
    if (!entry.registry) {
      toast.error("This typography preset is not available from the curated registry.");
      return;
    }

    const ok = appearanceStore.installTypography(entry.registry.typography, {
      source: "registry",
      registryId: entry.registry.id,
    });

    if (ok) {
      toast.success(`Installed ${entry.name}`);
    } else {
      toast.error(`Failed to install ${entry.name}`);
    }
  }

  async function applyTypography(entry: TypographyListEntry): Promise<void> {
    const ok = appearanceStore.applyTypographyPreset(entry.id);
    if (ok) {
      toast.success(`Applied ${entry.name}`);
    } else {
      toast.error(`Typography preset '${entry.id}' is unavailable.`);
    }
  }

  async function uninstallTypography(entry: TypographyListEntry): Promise<void> {
    if (entry.builtin) {
      toast.error("Built-in typography presets cannot be uninstalled.");
      return;
    }

    const ok = appearanceStore.uninstallTypography(entry.id);
    if (ok) {
      toast.success(`Removed ${entry.name}`);
      if (detailTypographyId === entry.id) {
        detailTypographyId = null;
      }
    } else {
      toast.error(`Failed to remove ${entry.name}`);
    }
  }

  function exportTypography(): void {
    const data = appearanceStore.exportTypography();
    const json = JSON.stringify(data, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `diaryx-typography-${appearanceStore.typographyPresetId}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function triggerLocalImport(): void {
    localFileInputRef?.click();
  }

  async function onLocalFileSelected(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";

    importingLocal = true;
    try {
      const text = await file.text();
      const parsed = JSON.parse(text);
      const typography = extractImportedTypography(parsed);
      if (!typography) {
        toast.error("Invalid typography file.");
        return;
      }

      const installed = appearanceStore.installTypography(typography, {
        source: "local",
        fileName: file.name,
      });
      if (!installed) {
        toast.error("Failed to import typography preset.");
        return;
      }

      toast.success(`Imported ${typography.name}`);
    } catch {
      toast.error("Failed to read typography file.");
    } finally {
      importingLocal = false;
    }
  }
</script>

<input
  type="file"
  accept=".json"
  class="hidden"
  bind:this={localFileInputRef}
  onchange={onLocalFileSelected}
/>

{#if detailEntry}
  {@const entry = detailEntry}
  <div class="flex flex-col h-full">
    <div class="flex items-center gap-2 px-3 py-2 border-b shrink-0">
      <Button
        variant="ghost"
        size="icon"
        class="size-7"
        onclick={() => (detailTypographyId = null)}
        aria-label="Back"
      >
        <ArrowLeft class="size-4" />
      </Button>
      <h3 class="text-sm font-medium truncate">{entry.name}</h3>
    </div>

    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
      <div class="space-y-1">
        <div class="flex items-center gap-2 flex-wrap">
          <Badge variant="secondary">v{entry.version}</Badge>
          {#if entry.active}
            <Badge variant="outline" class="text-emerald-600 dark:text-emerald-400">Active</Badge>
          {/if}
          {#if entry.builtin}
            <Badge variant="outline">Built-in</Badge>
          {/if}
        </div>
        <p class="text-xs text-muted-foreground">{entry.summary}</p>
        <p class="text-xs">{entry.description}</p>
      </div>

      <div class="space-y-2">
        <p class="text-xs font-medium">Preview</p>
        <div class="rounded-md border p-2.5 space-y-1.5">
          <p
            class="text-sm"
            style:font-family={FONT_FAMILY_MAP[entry.typography.settings.fontFamily]}
            style:font-size={`${entry.typography.settings.baseFontSize}px`}
            style:line-height={String(entry.typography.settings.lineHeight)}
            style:max-width={entry.typography.settings.contentWidth === "narrow"
              ? "55ch"
              : entry.typography.settings.contentWidth === "medium"
                ? "65ch"
                : entry.typography.settings.contentWidth === "wide"
                  ? "85ch"
                  : "none"}
          >
            The quick brown fox jumps over the lazy dog. Diaryx keeps typography
            readable while preserving your preferred writing rhythm.
          </p>
          <p class="text-[11px] text-muted-foreground">
            {entry.typography.settings.fontFamily} · {entry.typography.settings.baseFontSize}px · {entry.typography.settings.lineHeight} line-height · {entry.typography.settings.contentWidth} width
          </p>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-1.5 text-[11px]">
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Author</p>
          <p class="font-medium">{entry.author}</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">License</p>
          <p class="font-medium">{entry.license}</p>
        </div>
      </div>

      {#if entry.styles.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Styles</h4>
          <div class="flex flex-wrap gap-1">
            {#each entry.styles as style}
              <Badge variant="outline" class="text-[10px]">{style}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if entry.tags.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Tags</h4>
          <div class="flex flex-wrap gap-1">
            {#each entry.tags as tag}
              <Badge variant="secondary" class="text-[10px]">{tag}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      <div class="flex items-center gap-2 pt-1">
        {#if !entry.installed}
          <Button size="sm" onclick={() => installTypography(entry)}>
            <Download class="size-3.5 mr-1.5" />Install
          </Button>
        {:else}
          <Button
            size="sm"
            variant={entry.active ? "secondary" : "default"}
            onclick={() => applyTypography(entry)}
          >
            <Check class="size-3.5 mr-1.5" />{entry.active ? "Applied" : "Apply"}
          </Button>
          {#if !entry.builtin}
            <Button variant="outline" size="sm" onclick={() => uninstallTypography(entry)}>
              <Trash2 class="size-3.5 mr-1.5" />Uninstall
            </Button>
          {/if}
        {/if}
      </div>

      <div class="border-t pt-3 space-y-2">
        <div class="flex items-center justify-between gap-2">
          <h4 class="text-xs font-medium">Overrides</h4>
          <Button
            variant="ghost"
            size="sm"
            class="h-6 text-[11px]"
            onclick={() => appearanceStore.clearTypographyOverrides()}
            disabled={!entry.active}
          >
            Clear overrides
          </Button>
        </div>

        {#if entry.active}
          <div class="grid grid-cols-2 gap-1.5">
            <select
              class="h-7 rounded-md border bg-background px-2 text-xs"
              value={appearanceStore.typography.fontFamily}
              onchange={(e) =>
                appearanceStore.setFontFamily(
                  (e.target as HTMLSelectElement).value as FontFamily,
                )}
            >
              {#each fontOptions as opt}
                <option value={opt.value}>{opt.label}</option>
              {/each}
            </select>

            <select
              class="h-7 rounded-md border bg-background px-2 text-xs"
              value={appearanceStore.typography.baseFontSize}
              onchange={(e) =>
                appearanceStore.setBaseFontSize(
                  parseInt((e.target as HTMLSelectElement).value, 10),
                )}
            >
              {#each fontSizeOptions as size}
                <option value={size}>{size}px</option>
              {/each}
            </select>

            <select
              class="h-7 rounded-md border bg-background px-2 text-xs"
              value={appearanceStore.typography.lineHeight}
              onchange={(e) =>
                appearanceStore.setLineHeight(
                  parseFloat((e.target as HTMLSelectElement).value),
                )}
            >
              {#each lineHeightOptions as opt}
                <option value={opt.value}>{opt.label}</option>
              {/each}
            </select>

            <select
              class="h-7 rounded-md border bg-background px-2 text-xs"
              value={appearanceStore.layout.contentWidth}
              onchange={(e) =>
                appearanceStore.setContentWidth(
                  (e.target as HTMLSelectElement).value as ContentWidth,
                )}
            >
              {#each widthOptions as opt}
                <option value={opt.value}>{opt.label}</option>
              {/each}
            </select>
          </div>

          {#if Object.keys(appearanceStore.typographyOverrides).length > 0}
            <p class="text-[11px] text-muted-foreground">
              Overrides active: {Object.keys(appearanceStore.typographyOverrides).join(", ")}
            </p>
          {/if}
        {:else}
          <p class="text-[11px] text-muted-foreground">
            Apply this typography preset to adjust per-field overrides.
          </p>
        {/if}
      </div>

      {#if entry.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          onclick={() => openExternalUrl(entry.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}
    </div>
  </div>
{:else}
  <div class="flex flex-col h-full">
    <div class="flex-1 overflow-y-auto">
      {#if registryError}
        <div class="px-3 pt-2">
          <div class="rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-[11px] text-amber-700 dark:text-amber-300">
            {registryError}
          </div>
        </div>
      {/if}

      {#if registryLoading}
        <div class="flex items-center justify-center py-8 text-muted-foreground gap-2">
          <Loader2 class="size-4 animate-spin" />
          <span class="text-xs">Loading typography presets...</span>
        </div>
      {:else if filteredEntries.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No typography presets match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each filteredEntries as entry}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-muted-foreground"
              onclick={() => (detailTypographyId = entry.id)}
            >
              <div
                class="rounded border bg-muted/30 px-2 py-1.5 mb-1.5 overflow-hidden"
              >
                <p
                  class="text-[13px] leading-snug truncate"
                  style:font-family={FONT_FAMILY_MAP[entry.typography.settings.fontFamily]}
                >
                  The quick brown fox jumps over the lazy dog
                </p>
                <p class="text-[9px] text-muted-foreground mt-0.5">
                  {entry.typography.settings.fontFamily} · {entry.typography.settings.baseFontSize}px · {entry.typography.settings.lineHeight} lh
                </p>
              </div>
              <div class="flex items-center justify-between gap-2">
                <h3 class="text-xs font-medium truncate">{entry.name}</h3>
                <Badge variant="secondary" class="text-[9px] shrink-0">v{entry.version}</Badge>
              </div>
              <p class="text-[11px] text-muted-foreground mt-0.5 line-clamp-1">{entry.summary}</p>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                {#if entry.active}
                  <span class="text-[11px] text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1">
                    <Check class="size-2.5" />Active
                  </span>
                {:else if entry.installed}
                  <span class="text-[11px] text-muted-foreground">Installed</span>
                {:else if entry.registry}
                  <span class="text-[11px] text-muted-foreground">Curated</span>
                {:else}
                  <span class="text-[11px] text-muted-foreground">Unavailable</span>
                {/if}

                {#if !entry.installed}
                  <Button
                    variant="outline"
                    size="sm"
                    class="h-6 text-[11px] px-2"
                    onclick={(event) => {
                      event.stopPropagation();
                      void installTypography(entry);
                    }}
                    disabled={!entry.registry}
                  >
                    <Download class="size-3 mr-1" />Install
                  </Button>
                {:else}
                  <Button
                    variant="outline"
                    size="sm"
                    class="h-6 text-[11px] px-2"
                    onclick={(event) => {
                      event.stopPropagation();
                      void applyTypography(entry);
                    }}
                  >
                    <Check class="size-3 mr-1" />Apply
                  </Button>
                {/if}
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="px-3 py-2 space-y-2 border-t shrink-0">
      {#if filtersOpen}
        <div class="space-y-1.5">
          <select class="w-full h-7 rounded-md border bg-background px-2 text-xs" bind:value={categoryFilter}>
            {#each categories as category}
              <option value={category}>{category === "all" ? "All categories" : category}</option>
            {/each}
          </select>

          <select class="w-full h-7 rounded-md border bg-background px-2 text-xs" bind:value={styleFilter}>
            {#each styles as style}
              <option value={style}>{style === "all" ? "All styles" : style}</option>
            {/each}
          </select>

          <div class="flex gap-1.5">
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={sourceFilter}>
              <option value="all">All sources</option>
              <option value="installed">Installed</option>
              <option value="available">Curated</option>
            </select>
            <select class="flex-1 h-7 rounded-md border bg-background px-2 text-xs" bind:value={sortBy}>
              <option value="name">Name</option>
              <option value="recent">Recent</option>
              <option value="installed">Installed</option>
            </select>
          </div>
        </div>
      {/if}

      <div class="flex items-center gap-2">
        <div class="relative flex-1 min-w-0">
          <Search class="size-3.5 absolute left-2 top-2 text-muted-foreground" />
          <Input class="pl-7 h-7 text-xs" placeholder="Search typography" bind:value={search} />
        </div>

        <Button
          variant="outline"
          size="icon"
          class="size-7 shrink-0 {filtersOpen ? 'border-primary' : ''}"
          onclick={() => (filtersOpen = !filtersOpen)}
          aria-label="Toggle filters"
        >
          <SlidersHorizontal class="size-3.5" />
        </Button>

        <Button
          variant="outline"
          size="icon"
          class="size-7 shrink-0"
          onclick={triggerLocalImport}
          aria-label="Import local typography"
        >
          {#if importingLocal}
            <Loader2 class="size-3.5 animate-spin" />
          {:else}
            <Upload class="size-3.5" />
          {/if}
        </Button>

        <Button
          variant="outline"
          size="icon"
          class="size-7 shrink-0"
          onclick={exportTypography}
          aria-label="Export current typography"
        >
          <Download class="size-3.5" />
        </Button>
      </div>
    </div>
  </div>
{/if}
