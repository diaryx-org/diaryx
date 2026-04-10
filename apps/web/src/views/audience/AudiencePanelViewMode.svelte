<script lang="ts">
  /**
   * AudiencePanelViewMode — Multi-select audience list for filtering.
   *
   * Each audience shows as a checkbox row with a colored dot. Checking/unchecking
   * toggles the audience in templateContextStore.previewAudience[].
   */
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { Check, Globe } from "@lucide/svelte";

  interface Props {
    audiences: string[];
  }

  let { audiences }: Props = $props();

  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();

  let previewAudiences = $derived(templateContextStore.previewAudience);
  let isFiltering = $derived(previewAudiences !== null);

  function isChecked(name: string): boolean {
    return previewAudiences?.includes(name) ?? false;
  }

  function toggleAudience(name: string) {
    templateContextStore.togglePreviewAudience(name);
  }

  function clearAll() {
    templateContextStore.setPreviewAudience(null);
  }
</script>

<div class="view-mode">
  {#if audiences.length === 0}
    <div class="empty-state">No audiences defined yet.</div>
  {:else}
    <!-- Clear filter row -->
    <button
      type="button"
      class="audience-row"
      class:active={!isFiltering}
      onclick={clearAll}
    >
      <Globe class="size-3.5 text-muted-foreground" />
      <span class="audience-name">All audiences</span>
      {#if !isFiltering}
        <Check class="size-3.5 check-icon" />
      {/if}
    </button>

    <!-- Audience rows -->
    {#each audiences as audience}
      <button
        type="button"
        class="audience-row"
        class:active={isChecked(audience)}
        onclick={() => toggleAudience(audience)}
      >
        <span class="dot {getAudienceColor(audience, colorStore.audienceColors)}"></span>
        <span class="audience-name">{audience}</span>
        {#if isChecked(audience)}
          <Check class="size-3.5 check-icon" />
        {/if}
      </button>
    {/each}
  {/if}
</div>

<style>
  .view-mode {
    padding: 4px;
  }

  .empty-state {
    padding: 16px;
    text-align: center;
    font-size: 12px;
    color: var(--muted-foreground);
  }

  .audience-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    font-size: 13px;
    color: var(--foreground);
    background: transparent;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s ease;
    text-align: left;
  }

  .audience-row:hover {
    background: var(--muted);
  }

  .audience-row.active {
    background: color-mix(in oklch, var(--primary) 8%, transparent);
  }

  .audience-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .check-icon {
    flex-shrink: 0;
    color: var(--primary);
  }
</style>
