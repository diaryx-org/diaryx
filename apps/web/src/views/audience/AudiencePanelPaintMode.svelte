<script lang="ts">
  /**
   * AudiencePanelPaintMode — Brush selector for painting audiences onto
   * sidebar entries and editor text selections.
   *
   * User selects a brush (audience or "clear"), then:
   * - Clicking entries in LeftSidebar assigns/removes the audience
   * - Selecting text in the editor auto-applies a visibility mark/block
   */
  import { getAudiencePanelStore, CLEAR_BRUSH } from "$lib/stores/audiencePanelStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getAudienceColor } from "$lib/utils/audienceDotColor";
  import { Eraser } from "@lucide/svelte";
  import type { Api } from "$lib/backend";

  interface Props {
    audiences: string[];
    api: Api | null;
    rootPath: string;
  }

  let { audiences }: Props = $props();

  const panelStore = getAudiencePanelStore();
  const colorStore = getAudienceColorStore();

  function selectBrush(name: string) {
    if (panelStore.paintBrush === name) {
      panelStore.setBrush(null);
    } else {
      panelStore.setBrush(name);
    }
  }

  function selectClearBrush() {
    if (panelStore.paintBrush === CLEAR_BRUSH) {
      panelStore.setBrush(null);
    } else {
      panelStore.setBrush(CLEAR_BRUSH);
    }
  }
</script>

<div class="paint-mode">
  {#if audiences.length === 0}
    <div class="empty-state">
      Create an audience first, then use paint mode to assign it.
    </div>
  {:else}
    <div class="brush-hint">
      {#if panelStore.paintBrush}
        {#if panelStore.paintBrush === CLEAR_BRUSH}
          Click entries or select text to clear audiences.
        {:else}
          Click entries or select text to paint
          <span class="brush-name">
            <span class="dot {getAudienceColor(panelStore.paintBrush, colorStore.audienceColors)}"></span>
            {panelStore.paintBrush}
          </span>
        {/if}
      {:else}
        Pick a brush below, then click entries or select text.
      {/if}
    </div>

    <!-- Clear/eraser brush -->
    <button
      type="button"
      class="brush-row"
      class:active={panelStore.paintBrush === CLEAR_BRUSH}
      onclick={selectClearBrush}
    >
      <Eraser class="size-3.5 text-muted-foreground" />
      <span class="brush-name-text">Clear audience</span>
    </button>

    <!-- Audience brushes -->
    {#each audiences as audience}
      <button
        type="button"
        class="brush-row"
        class:active={panelStore.paintBrush === audience}
        onclick={() => selectBrush(audience)}
      >
        <span class="dot {getAudienceColor(audience, colorStore.audienceColors)}"></span>
        <span class="brush-name-text">{audience}</span>
      </button>
    {/each}
  {/if}
</div>

<style>
  .paint-mode {
    padding: 4px;
  }

  .empty-state {
    padding: 16px;
    text-align: center;
    font-size: 12px;
    color: var(--muted-foreground);
  }

  .brush-hint {
    padding: 6px 8px;
    font-size: 11px;
    color: var(--muted-foreground);
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .brush-name {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 500;
    color: var(--foreground);
  }

  .brush-row {
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

  .brush-row:hover {
    background: var(--muted);
  }

  .brush-row.active {
    background: color-mix(in oklch, var(--primary) 12%, transparent);
    outline: 1.5px solid color-mix(in oklch, var(--primary) 30%, transparent);
  }

  .brush-name-text {
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
</style>
