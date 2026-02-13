<script lang="ts">
  import type { Api } from "$lib/backend/api";
  import type { RustCrdtApi } from "$lib/crdt/rustCrdtApi";
  import * as Popover from "$lib/components/ui/popover";
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
  import { Button } from "$lib/components/ui/button";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { Plus, X, ArrowUpRight, Loader2 } from "@lucide/svelte";

  interface Props {
    /** null = not set (inherits), [] = explicitly empty, [...] = explicit tags */
    audience: string[] | null;
    entryPath: string;
    rootPath: string;
    api: Api | null;
    rustApi: RustCrdtApi | null;
    onChange: (value: string[] | null) => void;
  }

  let {
    audience,
    entryPath,
    rootPath,
    api,
    rustApi,
    onChange,
  }: Props = $props();

  const mobileState = getMobileState();

  // Combobox state
  let open = $state(false);
  let searchValue = $state("");
  let availableAudiences = $state<string[]>([]);

  // Inherited audience state
  let inheritedTags = $state<string[]>([]);
  let inheritedSourceTitle = $state<string | null>(null);
  let inheritedLoading = $state(false);

  // Resolve inherited audience by walking up part_of chain
  async function resolveInheritedAudience() {
    if (!rustApi) return;
    inheritedLoading = true;
    inheritedTags = [];
    inheritedSourceTitle = null;

    try {
      const docId = await rustApi.findDocIdByPath(entryPath);
      if (!docId) return;

      const meta = await rustApi.getFileById(docId);
      if (!meta?.part_of) return;

      // Walk up the parent chain
      let currentParentId: string | null = meta.part_of;
      const visited = new Set<string>();

      while (currentParentId) {
        if (visited.has(currentParentId)) break;
        visited.add(currentParentId);

        const parentMeta = await rustApi.getFileById(currentParentId);
        if (!parentMeta) break;

        if (parentMeta.audience && parentMeta.audience.length > 0) {
          inheritedTags = parentMeta.audience;
          inheritedSourceTitle = parentMeta.title || parentMeta.filename;
          return;
        }

        currentParentId = parentMeta.part_of;
      }
    } catch (e) {
      console.warn("[AudienceEditor] Failed to resolve inherited audience:", e);
    } finally {
      inheritedLoading = false;
    }
  }

  // Re-resolve when entry changes
  $effect(() => {
    if (entryPath && rustApi) {
      resolveInheritedAudience();
    }
  });

  // Load available audiences when combobox opens
  $effect(() => {
    if (open && api && rootPath) {
      loadAudiences();
    }
  });

  async function loadAudiences() {
    if (!api) return;
    try {
      availableAudiences = await api.getAvailableAudiences(rootPath);
    } catch (e) {
      console.warn("[AudienceEditor] Failed to load audiences:", e);
      availableAudiences = [];
    }
  }

  // Current explicit tags (empty array if null)
  const currentTags = $derived(audience ?? []);

  // Filter suggestions: exclude already-selected, match search
  const filteredSuggestions = $derived(
    availableAudiences
      .filter((a) => !currentTags.some((t) => t.toLowerCase() === a.toLowerCase()))
      .filter(
        (a) =>
          !searchValue.trim() ||
          a.toLowerCase().includes(searchValue.trim().toLowerCase()),
      ),
  );

  // Show "Create" option when typed text doesn't match any existing audience
  const showCreateOption = $derived(
    searchValue.trim().length > 0 &&
      !availableAudiences.some(
        (a) => a.toLowerCase() === searchValue.trim().toLowerCase(),
      ) &&
      !currentTags.some(
        (t) => t.toLowerCase() === searchValue.trim().toLowerCase(),
      ),
  );

  function addTag(tag: string) {
    const trimmed = tag.trim();
    if (!trimmed) return;

    if (audience === null) {
      // First tag on an inheriting entry — start explicit with just this tag
      onChange([trimmed]);
    } else {
      onChange([...audience, trimmed]);
    }
    open = false;
    searchValue = "";
  }

  function removeTag(index: number) {
    if (!audience) return;
    const newTags = [...audience];
    newTags.splice(index, 1);
    onChange(newTags);
  }

  function makeExplicit() {
    onChange([...inheritedTags]);
  }

  function revertToInherit() {
    onChange(null);
  }
</script>

{#snippet pickerContent()}
  <Command.Root class="rounded-lg border-none shadow-none" shouldFilter={false}>
    <Command.Input placeholder="Search audiences..." bind:value={searchValue} />
    <Command.List>
      {#if showCreateOption}
        <Command.Item value={"create:" + searchValue.trim()} onSelect={() => addTag(searchValue.trim())}>
          <Plus class="mr-2 size-4 shrink-0" />
          Create "{searchValue.trim()}"
        </Command.Item>
      {/if}
      {#if filteredSuggestions.length === 0 && !showCreateOption}
        <Command.Empty>No audiences found.</Command.Empty>
      {/if}
      {#each filteredSuggestions as tag (tag)}
        <Command.Item value={tag} onSelect={() => addTag(tag)}>
          {tag}
        </Command.Item>
      {/each}
    </Command.List>
  </Command.Root>
{/snippet}

{#snippet addButton()}
  <Button variant="ghost" size="sm" class="h-6 text-xs px-2">
    <Plus class="size-3 mr-1" />
    Add
  </Button>
{/snippet}

<div class="space-y-2">
  {#if audience === null}
    <!-- State 1: Inheriting from parent -->
    {#if inheritedLoading}
      <div class="flex items-center gap-2 text-xs text-muted-foreground">
        <Loader2 class="size-3 animate-spin" />
        <span>Resolving...</span>
      </div>
    {:else if inheritedTags.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each inheritedTags as tag}
          <span
            class="inline-flex items-center px-2 py-0.5 rounded-md text-xs bg-muted/50 text-muted-foreground border border-dashed border-border"
          >
            {tag}
          </span>
        {/each}
      </div>
      <p class="text-[10px] text-muted-foreground flex items-center gap-1">
        <ArrowUpRight class="size-2.5" />
        Inherited from {inheritedSourceTitle}
      </p>
      <div class="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          class="h-6 text-xs px-2"
          onclick={makeExplicit}
        >
          Make explicit
        </Button>
        {#if mobileState.isMobile}
          <Drawer.Root bind:open direction="bottom">
            <Drawer.Trigger>
              {@render addButton()}
            </Drawer.Trigger>
            <Drawer.Content>
              <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
                {@render pickerContent()}
              </div>
            </Drawer.Content>
          </Drawer.Root>
        {:else}
          <Popover.Root bind:open>
            <Popover.Trigger>
              {@render addButton()}
            </Popover.Trigger>
            <Popover.Content side="left" align="start" class="w-64 p-0">
              {@render pickerContent()}
            </Popover.Content>
          </Popover.Root>
        {/if}
      </div>
    {:else}
      <p class="text-xs text-muted-foreground">No audience set</p>
      {#if mobileState.isMobile}
        <Drawer.Root bind:open direction="bottom">
          <Drawer.Trigger>
            {@render addButton()}
          </Drawer.Trigger>
          <Drawer.Content>
            <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
              {@render pickerContent()}
            </div>
          </Drawer.Content>
        </Drawer.Root>
      {:else}
        <Popover.Root bind:open>
          <Popover.Trigger>
            {@render addButton()}
          </Popover.Trigger>
          <Popover.Content side="left" align="start" class="w-64 p-0">
            {@render pickerContent()}
          </Popover.Content>
        </Popover.Root>
      {/if}
    {/if}
  {:else if audience.length === 0}
    <!-- State 2: Explicitly empty -->
    <p class="text-xs text-muted-foreground">
      No audience — excluded from filtered exports
    </p>
    <div class="flex items-center gap-1">
      {#if mobileState.isMobile}
        <Drawer.Root bind:open direction="bottom">
          <Drawer.Trigger>
            {@render addButton()}
          </Drawer.Trigger>
          <Drawer.Content>
            <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
              {@render pickerContent()}
            </div>
          </Drawer.Content>
        </Drawer.Root>
      {:else}
        <Popover.Root bind:open>
          <Popover.Trigger>
            {@render addButton()}
          </Popover.Trigger>
          <Popover.Content side="left" align="start" class="w-64 p-0">
            {@render pickerContent()}
          </Popover.Content>
        </Popover.Root>
      {/if}
      <Button
        variant="ghost"
        size="sm"
        class="h-6 text-xs px-2"
        onclick={revertToInherit}
      >
        Inherit
      </Button>
    </div>
  {:else}
    <!-- State 3: Explicit tags -->
    <div class="flex flex-wrap gap-1">
      {#each audience as tag, i}
        <span
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-secondary text-secondary-foreground group/tag"
        >
          {tag}
          <button
            type="button"
            class="opacity-0 group-hover/tag:opacity-100 hover:text-destructive transition-opacity"
            onclick={() => removeTag(i)}
            aria-label="Remove {tag}"
          >
            <X class="size-3" />
          </button>
        </span>
      {/each}
    </div>
    <div class="flex items-center gap-1">
      {#if mobileState.isMobile}
        <Drawer.Root bind:open direction="bottom">
          <Drawer.Trigger>
            {@render addButton()}
          </Drawer.Trigger>
          <Drawer.Content>
            <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
              {@render pickerContent()}
            </div>
          </Drawer.Content>
        </Drawer.Root>
      {:else}
        <Popover.Root bind:open>
          <Popover.Trigger>
            {@render addButton()}
          </Popover.Trigger>
          <Popover.Content side="left" align="start" class="w-64 p-0">
            {@render pickerContent()}
          </Popover.Content>
        </Popover.Root>
      {/if}
      <Button
        variant="ghost"
        size="sm"
        class="h-6 text-xs px-2"
        onclick={revertToInherit}
      >
        Inherit
      </Button>
    </div>
  {/if}
</div>
