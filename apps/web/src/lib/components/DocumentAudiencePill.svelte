<script lang="ts">
  import type { Api } from "$lib/backend/api";
  import * as Popover from "$lib/components/ui/popover";
  import * as Command from "$lib/components/ui/command";
  import * as Drawer from "$lib/components/ui/drawer";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { Plus, Lock, ArrowUpRight, X } from "@lucide/svelte";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";

  interface Props {
    /** string[] = explicit tags, null = inheriting, [] = explicitly empty */
    audience: string[] | null;
    entryPath: string;
    rootPath: string;
    api: Api | null;
    onChange: (value: string[] | null) => void | Promise<void>;
    onOpenManager?: () => void;
  }

  let { audience, entryPath, rootPath, api, onChange, onOpenManager }: Props = $props();

  const mobileState = getMobileState();
  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();

  let open = $state(false);
  let searchValue = $state("");
  let availableAudiences = $state<string[]>([]);

  // Inherited audience state
  let inheritedTags = $state<string[]>([]);
  let inheritedSourceTitle = $state<string | null>(null);
  /** Whether this entry has a parent (part_of) and can inherit at all */
  let canInherit = $state(false);
  /** Whether this entry's audience came from the workspace default_audience config */
  let defaultAudienceApplied = $state(false);

  // Resolve inherited audience via the backend command
  async function resolveInheritedAudience() {
    if (!api) return;
    inheritedTags = [];
    inheritedSourceTitle = null;
    canInherit = false;
    defaultAudienceApplied = false;

    try {
      const result = await api.getEffectiveAudience(entryPath);
      canInherit = result.can_inherit;
      defaultAudienceApplied = result.default_audience_applied;
      if (result.inherited) {
        inheritedTags = result.tags;
        inheritedSourceTitle = result.source_title ?? null;
      } else if (result.default_audience_applied) {
        // Tags from default_audience — show them as "default" tags
        inheritedTags = result.tags;
      }
    } catch (e) {
      console.warn("[DocumentAudiencePill] Failed to resolve inherited audience:", e);
    }
  }

  // Re-resolve when entry changes
  $effect(() => {
    if (entryPath && api) {
      resolveInheritedAudience();
    }
  });

  const isInheriting = $derived(audience === null && canInherit);
  const isDefault = $derived(audience === null && !canInherit && defaultAudienceApplied);
  const displayTags = $derived(
    isInheriting ? inheritedTags :
    isDefault ? (inheritedTags.length > 0 ? inheritedTags : []) :
    (audience ?? [])
  );
  const isPrivate = $derived(displayTags.length === 0 && !defaultAudienceApplied);

  // Load available audiences when picker opens
  $effect(() => {
    if (open && api && rootPath) {
      api
        .getAvailableAudiences(rootPath)
        .then((a) => { availableAudiences = a; })
        .catch(() => { availableAudiences = []; });
    }
  });

  const filteredSuggestions = $derived(
    availableAudiences
      .filter((a) => !displayTags.some((t) => t.toLowerCase() === a.toLowerCase()))
      .filter(
        (a) =>
          !searchValue.trim() ||
          a.toLowerCase().includes(searchValue.trim().toLowerCase()),
      ),
  );

  const showCreateOption = $derived(
    searchValue.trim().length > 0 &&
      !availableAudiences.some(
        (a) => a.toLowerCase() === searchValue.trim().toLowerCase(),
      ) &&
      !displayTags.some(
        (t) => t.toLowerCase() === searchValue.trim().toLowerCase(),
      ),
  );

  async function addTag(tag: string) {
    const trimmed = tag.trim();
    if (!trimmed) return;
    const isNew = !availableAudiences.some(
      (a) => a.toLowerCase() === trimmed.toLowerCase(),
    );
    // audience null means inheriting — start an explicit list with this tag
    await onChange(audience === null ? [trimmed] : [...audience, trimmed]);
    if (isNew) {
      colorStore.assignColor(trimmed);
      templateContextStore.bumpAudiencesVersion();
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
</script>

{#snippet pickerContent()}
  <Command.Root
    class="rounded-lg border-none shadow-none"
    shouldFilter={false}
    aria-label="Select audience"
  >
    <Command.Input
      placeholder="Search audiences or create new..."
      bind:value={searchValue}
    />
    <Command.List role="listbox" aria-label="Audience suggestions">
      {#if showCreateOption}
        <Command.Item
          value={"create:" + searchValue.trim()}
          onSelect={() => addTag(searchValue.trim())}
        >
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
  {#if onOpenManager}
    <button
      type="button"
      class="w-full text-left text-xs text-muted-foreground hover:text-foreground transition-colors hover:underline px-3 py-1.5 border-t border-border"
      onclick={() => { open = false; onOpenManager?.(); }}
    >
      Manage audiences...
    </button>
  {/if}
{/snippet}

{#snippet trigger()}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="pill-area"
    role="button"
    tabindex="0"
    aria-label="Audience: {isPrivate ? 'Private' : isDefault ? displayTags.join(', ') + ' (default)' : isInheriting && inheritedTags.length > 0 ? 'Inherited: ' + inheritedTags.join(', ') : displayTags.join(', ')}. Click to edit."
    title="Who this entire journal entry is shared with"
  >
    {#if isInheriting && inheritedTags.length > 0}
      {#each inheritedTags as tag (tag)}
        <span class="pill-tag pill-inherited">
          <span class="pill-dot {getAudienceColor(tag, colorStore.audienceColors)}"></span>
          {tag}
        </span>
      {/each}
      <span class="pill-inherited-hint">
        <ArrowUpRight class="size-2.5" />
        {inheritedSourceTitle}
      </span>
      <span class="pill-add" aria-hidden="true">
        <Plus class="size-3" />
      </span>
    {:else if isDefault && displayTags.length > 0}
      {#each displayTags as tag (tag)}
        <span class="pill-tag pill-inherited">
          <span class="pill-dot {getAudienceColor(tag, colorStore.audienceColors)}"></span>
          {tag}
        </span>
      {/each}
      <span class="pill-inherited-hint">(default)</span>
      <span class="pill-add" aria-hidden="true">
        <Plus class="size-3" />
      </span>
    {:else if isPrivate}
      <span class="pill-private">
        <Lock class="size-3 shrink-0" />
        Private
      </span>
    {:else}
      {#each displayTags as tag, i (tag)}
        <span class="pill-tag">
          <span class="pill-dot {getAudienceColor(tag, colorStore.audienceColors)}"></span>
          {tag}
          <button
            type="button"
            class="pill-remove"
            onclick={(e: MouseEvent) => { e.stopPropagation(); removeTag(i); }}
            aria-label="Remove {tag}"
          >
            <X class="size-3" />
          </button>
        </span>
      {/each}
      <span class="pill-add" aria-hidden="true">
        <Plus class="size-3" />
      </span>
    {/if}
  </div>
{/snippet}

{#if mobileState.isMobile}
  <Drawer.Root bind:open direction="bottom">
    <Drawer.Trigger class="pill-wrapper">
      {@render trigger()}
    </Drawer.Trigger>
    <Drawer.Content>
      <div class="mx-auto w-full max-w-md px-4 pb-4 pt-2">
        {@render pickerContent()}
      </div>
    </Drawer.Content>
  </Drawer.Root>
{:else}
  <Popover.Root bind:open>
    <Popover.Trigger class="pill-wrapper">
      {@render trigger()}
    </Popover.Trigger>
    <Popover.Content side="bottom" align="start" class="w-64 p-0">
      {@render pickerContent()}
    </Popover.Content>
  </Popover.Root>
{/if}

<style>
  :global(.pill-wrapper) {
    display: block;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
  }

  .pill-area {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    padding: 2px 0;
    min-height: 44px;
    border-radius: 6px;
    transition: transform 0.15s ease, background 0.15s ease;
  }

  @media (min-width: 768px) {
    .pill-area {
      min-height: unset;
    }
  }

  .pill-area:hover {
    transform: scale(1.02);
  }

  /* "🔒 Private" empty state */
  .pill-private {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    min-height: 44px;
    border-radius: 4px;
    font-size: 12px;
    color: var(--muted-foreground);
    background: transparent;
    transition: background 0.15s ease;
  }

  @media (min-width: 768px) {
    .pill-private {
      min-height: unset;
    }
  }

  .pill-area:hover .pill-private {
    background: var(--muted);
  }

  /* Audience tag — identical to AudienceEditor.svelte State 3 spans */
  .pill-tag {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 12px;
    font-weight: 500;
    background: var(--secondary);
    color: var(--secondary-foreground);
  }

  /* Persistent audience color dot inside each pill */
  .pill-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    opacity: 0.85;
  }

  /* Inherited tag — dashed border to distinguish from explicit */
  .pill-inherited {
    background: color-mix(in srgb, var(--muted) 50%, transparent);
    border: 1px dashed var(--border);
  }

  /* Source hint next to inherited tags */
  .pill-inherited-hint {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    color: var(--muted-foreground);
    white-space: nowrap;
  }

  /* Compact "+" button after tags */
  .pill-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border-radius: 4px;
    color: var(--muted-foreground);
    background: transparent;
    transition: background 0.15s ease, color 0.15s ease;
  }

  @media (min-width: 768px) {
    .pill-add {
      width: 20px;
      height: 20px;
    }
  }

  .pill-area:hover .pill-add {
    background: var(--muted);
    color: var(--foreground);
  }

  /* Remove button inside explicit tags */
  .pill-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 8px;
    margin: -8px -6px -8px 0;
    border: none;
    background: none;
    color: var(--muted-foreground);
    cursor: pointer;
    transition: color 0.15s ease;
  }

  @media (min-width: 768px) {
    .pill-remove {
      padding: 0;
      margin: 0;
    }
  }

  .pill-remove:hover {
    color: var(--destructive);
  }
</style>
