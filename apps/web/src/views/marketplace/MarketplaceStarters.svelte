<script lang="ts">
  import {
    ArrowLeft,
    ExternalLink,
    FolderTree,
    Loader2,
    Search,
    SlidersHorizontal,
    Sparkles,
    Upload,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  import { openExternalUrl } from "$lib/billing";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    fetchStarterWorkspaceRegistry,
    normalizeStarterWorkspaceRegistryEntry,
    type StarterWorkspaceRegistry,
  } from "$lib/marketplace/starterWorkspaceRegistry";
  import type { StarterWorkspaceRegistryEntry } from "$lib/marketplace/types";
  import {
    fetchStarterWorkspaceZip,
  } from "$lib/marketplace/starterWorkspaceApply";
  import { getBackend } from "$lib/backend";
  import { getWorkspaceDirectoryPath } from "$lib/utils/path";
  import { useMarketplacePanel, isRecord } from "./useMarketplacePanel.svelte";

  interface DisplayStarter extends StarterWorkspaceRegistryEntry {
    source: "registry" | "local";
  }

  const panel = useMarketplacePanel<DisplayStarter>({
    async fetchItems() {
      const registry = await fetchStarterWorkspaceRegistry();
      return (registry as StarterWorkspaceRegistry).starters.map(
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
      if (!payload) throw new Error("Starter file is empty");

      if (isRecord(payload) && Array.isArray(payload.starters)) {
        return payload.starters.map(
          (entry) => ({ ...normalizeStarterWorkspaceRegistryEntry(entry), source: "local" as const }),
        );
      }
      if (isRecord(payload) && "starter" in payload) {
        return [{ ...normalizeStarterWorkspaceRegistryEntry(payload.starter), source: "local" as const }];
      }
      return [{ ...normalizeStarterWorkspaceRegistryEntry(payload), source: "local" as const }];
    },
  });

  let applyProgress = $state<string | null>(null);

  async function handleApply(starter: DisplayStarter): Promise<void> {
    panel.addActiveId(starter.id);
    applyProgress = "Downloading starter...";

    try {
      const zipBlob = await fetchStarterWorkspaceZip(starter);
      const zipFile = new File([zipBlob], "starter.zip", { type: "application/zip" });

      applyProgress = "Importing starter content...";
      const backend = await getBackend();
      const workspaceDir = getWorkspaceDirectoryPath(backend.getWorkspacePath());

      const result = await backend.importFromZip(
        zipFile,
        workspaceDir,
        (uploaded, total) => {
          if (total > 0) {
            applyProgress = `Importing... ${Math.round((uploaded / total) * 100)}%`;
          }
        },
      );

      toast.success(
        `Applied starter "${starter.name}": ${result.files_imported} files imported`,
      );
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to apply starter");
    } finally {
      panel.removeActiveId(starter.id);
      applyProgress = null;
    }
  }

  async function handleLocalFileSelected(event: Event): Promise<void> {
    try {
      const items = await panel.onLocalFileSelected(event);
      if (items.length > 0) {
        toast.success(`Imported ${items.length} starter${items.length === 1 ? "" : "s"}`);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Invalid starter file");
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

{#if panel.detailItem}
  {@const starter = panel.detailItem}
  {@const applying = panel.isActive(starter.id)}
  <div class="flex flex-col h-full">
    <div class="flex items-center gap-2 px-3 py-2 border-b shrink-0">
      <Button variant="ghost" size="icon" class="size-7" onclick={() => (panel.detailId = null)} aria-label="Back">
        <ArrowLeft class="size-4" />
      </Button>
      <h3 class="text-sm font-medium truncate">{starter.name}</h3>
    </div>

    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
      <div class="space-y-1">
        <div class="flex items-center gap-2 flex-wrap">
          <Badge variant="secondary">v{starter.version}</Badge>
          <Badge variant="outline">{starter.source === "registry" ? "Curated" : "Local"}</Badge>
        </div>
        <p class="text-xs text-muted-foreground">{starter.summary}</p>
        <p class="text-xs">{starter.description}</p>
      </div>

      <div class="grid grid-cols-2 gap-1.5 text-[11px]">
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Files</p>
          <p class="font-medium">{starter.file_count}</p>
        </div>
        <div class="rounded-md border p-1.5">
          <p class="text-muted-foreground">Templates</p>
          <p class="font-medium">{starter.includes_templates ? "Yes" : "No"}</p>
        </div>
      </div>

      {#if starter.bundle_id}
        <div class="rounded-md border p-2 text-[11px] space-y-1">
          <p class="font-medium">Linked bundle</p>
          <p class="text-muted-foreground">{starter.bundle_id}</p>
        </div>
      {/if}

      {#if starter.file_count > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Contents</h4>
          <p class="text-[11px] text-muted-foreground">
            {starter.file_count} file{starter.file_count === 1 ? "" : "s"}{#if starter.includes_templates}, including templates{/if}
          </p>
        </div>
      {/if}

      {#if starter.tags.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Tags</h4>
          <div class="flex flex-wrap gap-1">
            {#each starter.tags as tag}
              <Badge variant="secondary" class="text-[10px]">{tag}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if starter.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          onclick={() => openExternalUrl(starter.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}

      <div class="flex items-center gap-2 pt-1">
        <Button size="sm" onclick={() => handleApply(starter)} disabled={applying}>
          {#if applying}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />{applyProgress ?? "Applying..."}
          {:else}
            <Sparkles class="size-3.5 mr-1.5" />Apply to Workspace
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
          <span class="text-xs">Loading starters...</span>
        </div>
      {:else if panel.filteredItems.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No starters match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each panel.filteredItems as starter}
            {@const applying = panel.isActive(starter.id)}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-muted-foreground"
              onclick={() => (panel.detailId = starter.id)}
            >
              <div class="flex items-center gap-2 mb-1">
                <FolderTree class="size-3.5 text-muted-foreground shrink-0" />
                <h3 class="text-xs font-medium truncate">{starter.name}</h3>
                <Badge variant="secondary" class="text-[9px] shrink-0 ml-auto">v{starter.version}</Badge>
              </div>
              <p class="text-[11px] text-muted-foreground mt-0.5 line-clamp-1">{starter.summary}</p>
              <div class="flex items-center gap-2 mt-1 text-[9px] text-muted-foreground">
                <span>{starter.file_count} file{starter.file_count === 1 ? "" : "s"}</span>
                {#if starter.includes_templates}
                  <span>+ templates</span>
                {/if}
                {#if starter.bundle_id}
                  <span>+ bundle</span>
                {/if}
              </div>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[11px] text-muted-foreground">
                  {starter.source === "registry" ? "Curated" : "Local"}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  class="h-6 text-[11px] px-2"
                  onclick={(event) => {
                    event.stopPropagation();
                    handleApply(starter);
                  }}
                  disabled={applying}
                >
                  {#if applying}
                    <Loader2 class="size-3 mr-1 animate-spin" />
                  {:else}
                    <Sparkles class="size-3 mr-1" />
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
          <Input class="pl-7 h-7 text-xs" placeholder="Search starters" bind:value={panel.search} />
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

        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={panel.triggerLocalImport} aria-label="Import local starter">
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
