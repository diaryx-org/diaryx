<script lang="ts">
  import {
    ArrowLeft,
    Download,
    ExternalLink,
    FileText,
    Loader2,
    Search,
    SlidersHorizontal,
    Upload,
  } from "@lucide/svelte";
  import { toast } from "svelte-sonner";

  import { openExternalUrl } from "$lib/billing";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    fetchTemplateRegistry,
    normalizeTemplateRegistryEntry,
    type TemplateRegistry,
  } from "$lib/marketplace/templateRegistry";
  import type { TemplateRegistryEntry } from "$lib/marketplace/types";
  import {
    installMarketplaceTemplate,
    type TemplateInstallRuntime,
  } from "$lib/marketplace/templateInstall";
  import { getBackend, createApi } from "$lib/backend";
  import { useMarketplacePanel, isRecord } from "./useMarketplacePanel.svelte";

  interface DisplayTemplate extends TemplateRegistryEntry {
    source: "registry" | "local";
  }

  const panel = useMarketplacePanel<DisplayTemplate>({
    async fetchItems() {
      const registry = await fetchTemplateRegistry();
      return (registry as TemplateRegistry).templates.map(
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
      if (!payload) throw new Error("Template file is empty");

      if (isRecord(payload) && Array.isArray(payload.templates)) {
        return payload.templates.map(
          (entry) => ({ ...normalizeTemplateRegistryEntry(entry), source: "local" as const }),
        );
      }
      if (isRecord(payload) && "template" in payload) {
        return [{ ...normalizeTemplateRegistryEntry(payload.template), source: "local" as const }];
      }
      return [{ ...normalizeTemplateRegistryEntry(payload), source: "local" as const }];
    },
  });

  async function handleInstall(template: DisplayTemplate): Promise<void> {
    panel.addActiveId(template.id);
    try {
      const backend = await getBackend();
      const api = createApi(backend);
      const workspaceDir = backend.getWorkspacePath()
        .replace(/\/index\.md$/, "")
        .replace(/\/README\.md$/, "");

      const runtime: TemplateInstallRuntime = {
        saveTemplate: (name, content, wsPath) => api.saveTemplate(name, content, wsPath),
        listTemplateNames: async () => {
          const list = await api.listTemplates(workspaceDir);
          return list.map((t) => t.name);
        },
      };

      await installMarketplaceTemplate(template, workspaceDir, runtime);
      toast.success(`Installed template "${template.name}"`);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to install template");
    } finally {
      panel.removeActiveId(template.id);
    }
  }

  async function handleLocalFileSelected(event: Event): Promise<void> {
    try {
      const items = await panel.onLocalFileSelected(event);
      if (items.length > 0) {
        toast.success(`Imported ${items.length} template${items.length === 1 ? "" : "s"}`);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Invalid template file");
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
  {@const template = panel.detailItem}
  {@const installing = panel.isActive(template.id)}
  <div class="flex flex-col h-full">
    <div class="flex items-center gap-2 px-3 py-2 border-b shrink-0">
      <Button variant="ghost" size="icon" class="size-7" onclick={() => (panel.detailId = null)} aria-label="Back">
        <ArrowLeft class="size-4" />
      </Button>
      <h3 class="text-sm font-medium truncate">{template.name}</h3>
    </div>

    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-3">
      <div class="space-y-1">
        <div class="flex items-center gap-2 flex-wrap">
          <Badge variant="secondary">v{template.version}</Badge>
          <Badge variant="outline">{template.source === "registry" ? "Curated" : "Local"}</Badge>
        </div>
        <p class="text-xs text-muted-foreground">{template.summary}</p>
        <p class="text-xs">{template.description}</p>
      </div>

      {#if template.template_variables.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Template variables</h4>
          <div class="flex flex-wrap gap-1">
            {#each template.template_variables as variable}
              <Badge variant="secondary" class="text-[10px] font-mono">{`{{${variable}}}`}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if template.preview}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Preview</h4>
          <div class="rounded-md border p-2 text-[11px] text-muted-foreground whitespace-pre-wrap font-mono max-h-[200px] overflow-y-auto">
            {template.preview}
          </div>
        </div>
      {/if}

      {#if template.tags.length > 0}
        <div class="space-y-1">
          <h4 class="text-xs font-medium">Tags</h4>
          <div class="flex flex-wrap gap-1">
            {#each template.tags as tag}
              <Badge variant="secondary" class="text-[10px]">{tag}</Badge>
            {/each}
          </div>
        </div>
      {/if}

      {#if template.repository}
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs text-primary hover:underline"
          onclick={() => openExternalUrl(template.repository!)}
        >
          Repository <ExternalLink class="size-3" />
        </button>
      {/if}

      <div class="flex items-center gap-2 pt-1">
        <Button size="sm" onclick={() => handleInstall(template)} disabled={installing}>
          {#if installing}
            <Loader2 class="size-3.5 mr-1.5 animate-spin" />Installing...
          {:else}
            <Download class="size-3.5 mr-1.5" />Install Template
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
          <span class="text-xs">Loading templates...</span>
        </div>
      {:else if panel.filteredItems.length === 0}
        <div class="px-3 py-4 text-xs text-muted-foreground">No templates match your filters.</div>
      {:else}
        <div class="p-2 space-y-1.5">
          {#each panel.filteredItems as template}
            {@const installing = panel.isActive(template.id)}
            <button
              type="button"
              class="w-full text-left rounded-md border p-2.5 transition hover:border-muted-foreground"
              onclick={() => (panel.detailId = template.id)}
            >
              <div class="flex items-center gap-2 mb-1">
                <FileText class="size-3.5 text-muted-foreground shrink-0" />
                <h3 class="text-xs font-medium truncate">{template.name}</h3>
                <Badge variant="secondary" class="text-[9px] shrink-0 ml-auto">v{template.version}</Badge>
              </div>
              <p class="text-[11px] text-muted-foreground mt-0.5 line-clamp-1">{template.summary}</p>
              {#if template.template_variables.length > 0}
                <div class="flex flex-wrap gap-0.5 mt-1">
                  {#each template.template_variables.slice(0, 4) as variable}
                    <span class="text-[9px] px-1 py-0.5 rounded bg-muted text-muted-foreground font-mono">{`{{${variable}}}`}</span>
                  {/each}
                  {#if template.template_variables.length > 4}
                    <span class="text-[9px] px-1 py-0.5 text-muted-foreground">+{template.template_variables.length - 4}</span>
                  {/if}
                </div>
              {/if}
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[11px] text-muted-foreground">
                  {template.source === "registry" ? "Curated" : "Local"}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  class="h-6 text-[11px] px-2"
                  onclick={(event) => {
                    event.stopPropagation();
                    handleInstall(template);
                  }}
                  disabled={installing}
                >
                  {#if installing}
                    <Loader2 class="size-3 mr-1 animate-spin" />
                  {:else}
                    <Download class="size-3 mr-1" />
                  {/if}
                  Install
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
          <Input class="pl-7 h-7 text-xs" placeholder="Search templates" bind:value={panel.search} />
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

        <Button variant="outline" size="icon" class="size-7 shrink-0" onclick={panel.triggerLocalImport} aria-label="Import local template">
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
