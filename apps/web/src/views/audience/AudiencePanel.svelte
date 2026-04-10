<script lang="ts">
  /**
   * AudiencePanel — Floating bottom-center panel for audience view/paint modes.
   *
   * Replaces AudienceFilter (sidebar dropdown), AudienceManager (modal), and
   * VisibilityPicker (bubble menu). Activated by clicking audience color dots.
   * Two modes: View (multi-select filter) and Paint (brush entries/text).
   *
   * New audiences are created from Paint mode as transient brushes — they only
   * persist once the user actually paints something with them.
   */
  import { getAudiencePanelStore } from "$lib/stores/audiencePanelStore.svelte";
  import { getTemplateContextStore } from "$lib/stores/templateContextStore.svelte";
  import { getAudienceColorStore } from "$lib/stores/audienceColorStore.svelte";
  import { getMobileState } from "$lib/hooks/useMobile.svelte";
  import { X, Eye, Paintbrush } from "@lucide/svelte";
  import AudiencePanelViewMode from "./AudiencePanelViewMode.svelte";
  import AudiencePanelPaintMode from "./AudiencePanelPaintMode.svelte";
  import type { Api } from "$lib/backend";

  interface Props {
    api: Api | null;
    rootPath: string;
  }

  let { api, rootPath }: Props = $props();

  const panelStore = getAudiencePanelStore();
  const templateContextStore = getTemplateContextStore();
  const colorStore = getAudienceColorStore();
  const mobileState = getMobileState();

  let audiences = $state<string[]>([]);

  async function loadAudiences() {
    if (!api || !rootPath) {
      audiences = [];
      return;
    }
    try {
      audiences = await api.getAvailableAudiences(rootPath);
      for (const name of audiences) colorStore.assignColor(name);
      // If a transient brush has now appeared on disk, demote it from
      // transient to "real" so the panel doesn't double-track it.
      const t = panelStore.transientAudience;
      if (t && audiences.includes(t)) {
        panelStore.confirmTransientPersisted();
      }
      // Drop colors for audiences that no longer exist anywhere in the
      // workspace. Keep the active transient (if any) since it has a color
      // assigned but isn't yet on disk.
      const keep = new Set(audiences);
      const stillTransient = panelStore.transientAudience;
      if (stillTransient) keep.add(stillTransient);
      colorStore.pruneTo(keep);
    } catch (e) {
      console.warn("[AudiencePanel] Failed to load audiences:", e);
      audiences = [];
    }
  }

  $effect(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    templateContextStore.audiencesVersion;
    if (rootPath && panelStore.panelOpen) {
      loadAudiences();
    }
  });

  // Load audiences when panel opens
  $effect(() => {
    if (panelStore.panelOpen) {
      loadAudiences();
    }
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && panelStore.panelOpen) {
      event.preventDefault();
      event.stopPropagation();
      panelStore.closePanel();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if panelStore.panelOpen}
  <div
    class="audience-panel"
    class:mobile={mobileState.isMobile}
    role="dialog"
    aria-label="Audience panel"
  >
    <!-- Header -->
    <div class="panel-header">
      <div class="mode-tabs">
        <button
          type="button"
          class="mode-tab"
          class:active={panelStore.mode === "view"}
          onclick={() => panelStore.setMode("view")}
        >
          <Eye class="size-3.5" />
          View
        </button>
        <button
          type="button"
          class="mode-tab"
          class:active={panelStore.mode === "paint"}
          onclick={() => panelStore.setMode("paint")}
        >
          <Paintbrush class="size-3.5" />
          Paint
        </button>
      </div>
      <button
        type="button"
        class="close-btn"
        onclick={() => panelStore.closePanel()}
        aria-label="Close audience panel"
      >
        <X class="size-4" />
      </button>
    </div>

    <!-- Body -->
    <div class="panel-body">
      {#if panelStore.mode === "view"}
        <AudiencePanelViewMode {audiences} />
      {:else}
        <AudiencePanelPaintMode {audiences} {api} {rootPath} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .audience-panel {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 40;
    width: 380px;
    max-width: calc(100vw - 32px);
    background: var(--popover);
    color: var(--popover-foreground);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    max-height: 400px;
  }

  .audience-panel.mobile {
    bottom: 0;
    left: 0;
    right: 0;
    transform: none;
    width: 100%;
    max-width: 100%;
    border-radius: 12px 12px 0 0;
    border-bottom: none;
    z-index: 50;
    max-height: 50vh;
    padding-bottom: env(safe-area-inset-bottom);
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 8px 8px 4px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .mode-tabs {
    display: flex;
    gap: 2px;
    padding: 2px;
    background: color-mix(in oklch, var(--muted) 50%, transparent);
    border-radius: 6px;
  }

  .mode-tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    font-size: 12px;
    font-weight: 500;
    color: var(--muted-foreground);
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .mode-tab:hover {
    color: var(--foreground);
  }

  .mode-tab.active {
    color: var(--foreground);
    background: var(--background);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--muted-foreground);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .close-btn:hover {
    background: var(--muted);
    color: var(--foreground);
  }

  .panel-body {
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }
</style>
