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
  import { getAppearanceStore } from "$lib/stores/appearance.svelte";
  import type { ThemeDefinition } from "$lib/stores/appearance.types";
  import { fetchThemeRegistry, type ThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { ThemeRegistryEntry } from "$lib/marketplace/types";
  import AccentHuePicker from "$lib/settings/AccentHuePicker.svelte";

  type SourceFilter = "all" | "installed" | "available";
  type SortBy = "name" | "recent";

  interface ThemeListEntry {
    id: string;
    theme: ThemeDefinition;
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
    registry: ThemeRegistryEntry | null;
    installedAt: number | null;
    publishedAt: number | null;
  }

  const appearanceStore = getAppearanceStore();

  let registryThemes = $state<ThemeRegistryEntry[]>([]);
  let registryLoading = $state(true);
  let registryError = $state<string | null>(null);

  let search = $state("");
  let filtersOpen = $state(false);
  let sourceFilter = $state<SourceFilter>("all");
  let categoryFilter = $state("all");
  let styleFilter = $state("all");
  let sortBy = $state<SortBy>("name");

  let importingLocal = $state(false);
  let detailThemeId = $state<string | null>(null);
  let localFileInputRef = $state<HTMLInputElement | null>(null);

  function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
  }

  function isThemeDefinition(value: unknown): value is ThemeDefinition {
    if (!isRecord(value)) return false;
    return (
      typeof value.id === "string" &&
      typeof value.name === "string" &&
      value.version === 1 &&
      isRecord(value.colors) &&
      isRecord(value.colors.light) &&
      isRecord(value.colors.dark)
    );
  }

  function extractImportedTheme(value: unknown): ThemeDefinition | null {
    if (isRecord(value) && "theme" in value && isThemeDefinition(value.theme)) {
      return value.theme;
    }

    if (isThemeDefinition(value)) {
      return value;
    }

    return null;
  }

  function themeSwatches(theme: ThemeDefinition, mode: "light" | "dark"): string[] {
    const palette = mode === "dark" ? theme.colors.dark : theme.colors.light;
    return [
      palette.background,
      palette.primary,
      palette.accent,
      palette.muted,
      palette.foreground,
    ];
  }

  async function loadRegistry(): Promise<void> {
    registryLoading = true;
    registryError = null;
    try {
      const registry: ThemeRegistry = await fetchThemeRegistry();
      registryThemes = registry.themes;
    } catch (error) {
      registryThemes = [];
      registryError =
        error instanceof Error ? error.message : "Failed to load theme registry";
    } finally {
      registryLoading = false;
    }
  }

  $effect(() => {
    loadRegistry();
  });

  const allEntries = $derived.by(() => {
    const installedThemes = appearanceStore.listThemes();
    const installedById = new Map(installedThemes.map((entry) => [entry.theme.id, entry]));
    const registryById = new Map(registryThemes.map((entry) => [entry.id, entry]));

    const ids = new Set<string>([
      ...Array.from(installedById.keys()),
      ...Array.from(registryById.keys()),
    ]);

    const entries: ThemeListEntry[] = [];

    for (const id of ids) {
      const installed = installedById.get(id);
      const registry = registryById.get(id) ?? null;
      const theme = installed?.theme ?? registry?.theme;
      if (!theme) continue;

      const installedAt =
        typeof installed?.source.installedAt === "number"
          ? installed.source.installedAt
          : null;
      const publishedAt = registry?.artifact?.published_at
        ? Date.parse(registry.artifact.published_at) || null
        : null;

      entries.push({
        id,
        theme,
        name: registry?.name ?? theme.name,
        version: registry?.version ?? `${theme.version}`,
        summary:
          registry?.summary ??
          theme.description ??
          "A custom Diaryx theme.",
        description:
          registry?.description ??
          theme.description ??
          "No description provided.",
        author: registry?.author ?? theme.author ?? "Unknown",
        license: registry?.license ?? "Custom",
        repository: registry?.repository ?? null,
        categories: registry?.categories ?? [],
        tags: registry?.tags ?? [],
        styles: registry?.styles ?? [],
        screenshots: registry?.screenshots ?? [],
        installed: !!installed,
        builtin: installed?.builtin ?? false,
        active: appearanceStore.presetId === id,
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
      if (sortBy === "name") return a.name.localeCompare(b.name);
      const aTs = a.publishedAt ?? a.installedAt ?? 0;
      const bTs = b.publishedAt ?? b.installedAt ?? 0;
      return bTs - aTs;
    });

    return filtered;
  });

  const detailEntry = $derived.by(() => {
    if (!detailThemeId) return null;
    return allEntries.find((entry) => entry.id === detailThemeId) ?? null;
  });

  async function installTheme(entry: ThemeListEntry): Promise<void> {
    if (!entry.registry) {
      toast.error("This theme is not available from the curated registry.");
      return;
    }

    const ok = appearanceStore.installTheme(entry.registry.theme, {
      source: "registry",
      registryId: entry.registry.id,
    });

    if (ok) {
      toast.success(`Installed ${entry.name}`);
    } else {
      toast.error(`Failed to install ${entry.name}`);
    }
  }

  async function applyTheme(entry: ThemeListEntry): Promise<void> {
    const ok = appearanceStore.applyTheme(entry.id);
    if (ok) {
      toast.success(`Applied ${entry.name}`);
    } else {
      toast.error(`Theme '${entry.id}' is unavailable.`);
    }
  }

  async function uninstallTheme(entry: ThemeListEntry): Promise<void> {
    if (entry.builtin) {
      toast.error("Built-in themes cannot be uninstalled.");
      return;
    }

    const ok = appearanceStore.uninstallTheme(entry.id);
    if (ok) {
      toast.success(`Removed ${entry.name}`);
      if (detailThemeId === entry.id) {
        detailThemeId = null;
      }
    } else {
      toast.error(`Failed to remove ${entry.name}`);
    }
  }

  function exportTheme(): void {
    const data = appearanceStore.exportTheme();
    const json = JSON.stringify(data, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `diaryx-theme-${appearanceStore.presetId}.json`;
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
      const theme = extractImportedTheme(parsed);
      if (!theme) {
        toast.error("Invalid theme file.");
        return;
      }

      const installed = appearanceStore.installTheme(theme, {
        source: "local",
        fileName: file.name,
      });
      if (!installed) {
        toast.error("Failed to import theme.");
        return;
      }

      toast.success(`Imported ${theme.name}`);
    } catch {
      toast.error("Failed to read theme file.");
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
      <Button variant="ghost" size="icon" class="size-7" onclick={() => (detailThemeId = null)} aria-label="Back">
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
        <div class="space-y-1">
          <div class="flex items-center gap-1">
            {#each themeSwatches(entry.theme, "light") as swatch}
              <span class="size-4 rounded-full border border-black/10" style:background={swatch}></span>
            {/each}
          </div>
          <div class="flex items-center gap-1">
            {#each themeSwatches(entry.theme, "dark") as swatch}
              <span class="size-4 rounded-full border border-white/20" style:background={swatch}></span>
            {/each}
          </div>
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

      <div class="space-y-1.5">
        <h4 class="text-xs font-medium">Accent color</h4>
        <AccentHuePicker
          value={appearanceStore.accentHue}
          onchange={(hue) => appearanceStore.setAccentHue(hue)}
        />
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

      <div class="flex items-center gap-2 pt-1">
        {#if !entry.installed}
          <Button size="sm" onclick={() => installTheme(entry)}>
            <Download class="size-3.5 mr-1.5" />Install
          </Button>
        {:else}
          <Button size="sm" variant={entry.active ? "secondary" : "default"} onclick={() => applyTheme(entry)}>
            <Check class="size-3.5 mr-1.5" />{entry.active ? "Applied" : "Apply"}
          </Button>
          {#if !entry.builtin}
            <Button variant="outline" size="sm" onclick={() => uninstallTheme(entry)}>
              <Trash2 class="size-3.5 mr-1.5" />Uninstall
            </Button>
          {/if}
        {/if}
      </div>
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
          <span class="text-xs">Loading themes...</span>
        </div>
      {:else if filteredEntries.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No themes match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each filteredEntries as entry}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-muted-foreground"
              onclick={() => (detailThemeId = entry.id)}
            >
              <div class="flex gap-1.5 mb-1.5">
                <div class="flex items-center gap-px">
                  {#each themeSwatches(entry.theme, "light") as swatch}
                    <span class="size-3.5 first:rounded-l-sm last:rounded-r-sm border border-black/10" style:background={swatch}></span>
                  {/each}
                </div>
                <div class="flex items-center gap-px">
                  {#each themeSwatches(entry.theme, "dark") as swatch}
                    <span class="size-3.5 first:rounded-l-sm last:rounded-r-sm border border-white/20" style:background={swatch}></span>
                  {/each}
                </div>
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
                      void installTheme(entry);
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
                      void applyTheme(entry);
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
            </select>
          </div>
        </div>
      {/if}

      <div class="flex items-center gap-2">
        <div class="relative flex-1 min-w-0">
          <Search class="size-3.5 absolute left-2 top-2 text-muted-foreground" />
          <Input class="pl-7 h-7 text-xs" placeholder="Search themes" bind:value={search} />
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

        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={triggerLocalImport} aria-label="Import local theme">
          {#if importingLocal}
            <Loader2 class="size-3.5 animate-spin" />
          {:else}
            <Upload class="size-3.5" />
          {/if}
        </Button>

        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={exportTheme} aria-label="Export current theme">
          <Download class="size-3.5" />
        </Button>
      </div>
    </div>
  </div>
{/if}
