<script lang="ts">
  import type { Api } from "$lib/backend/api";
  import { Plus, Lock, ArrowUpRight, X } from "@lucide/svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { getAudiencePanelStore } from "$lib/stores/audiencePanelStore.svelte";

  interface Props {
    /** string[] = explicit tags, null = inheriting, [] = explicitly empty */
    audience: string[] | null;
    entryPath: string;
    api: Api | null;
    onChange: (value: string[] | null) => void | Promise<void>;
  }

  let { audience, entryPath, api, onChange }: Props = $props();

  const colorStore = getAudienceColorStore();
  const audiencePanelStore = getAudiencePanelStore();

  // Inherited audience state
  let inheritedTags = $state<string[]>([]);
  let inheritedSourceTitle = $state<string | null>(null);
  let canInherit = $state(false);
  let defaultAudienceApplied = $state(false);

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
        inheritedTags = result.tags;
      }
    } catch (e) {
      console.warn("[DocumentAudiencePill] Failed to resolve inherited audience:", e);
    }
  }

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

  function removeTag(index: number) {
    if (!audience) return;
    const newTags = [...audience];
    newTags.splice(index, 1);
    onChange(newTags);
  }

  function handleOpenPanel() {
    audiencePanelStore.openPanel("paint");
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="pill-area"
  role="button"
  tabindex="0"
  onclick={handleOpenPanel}
  onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") handleOpenPanel(); }}
  aria-label="Audience: {isPrivate ? 'Private' : displayTags.join(', ')}. Click to edit."
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

<style>
  .pill-area {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    padding: 2px 0;
    min-height: 44px;
    border-radius: 6px;
    cursor: pointer;
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

  .pill-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    opacity: 0.85;
  }

  .pill-inherited {
    background: color-mix(in srgb, var(--muted) 50%, transparent);
    border: 1px dashed var(--border);
  }

  .pill-inherited-hint {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    color: var(--muted-foreground);
    white-space: nowrap;
  }

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
