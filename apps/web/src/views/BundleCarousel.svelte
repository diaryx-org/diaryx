<script lang="ts">
  /**
   * BundleCarousel — fullscreen bundle picker with live-rendered iframe previews.
   *
   * Each bundle is shown as a scaled-down iframe running the preview route.
   * Left/right arrows navigate between bundles. On selection, the preview
   * zooms to fill the viewport (app launch animation).
   */
  import { onMount } from "svelte";
  import { ChevronLeft, ChevronRight, Loader2, ChevronDown, Upload, X } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "svelte-sonner";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";

  export interface BundleSelectInfo {
    bundle: BundleRegistryEntry;
    launchRect: DOMRect | null;
    previewUrl: string;
    pluginOverrides?: PluginOverride[];
  }

  /** A local .wasm override for a plugin in the bundle */
  export interface PluginOverride {
    /** The plugin ID being replaced (or "__new__" for an addition) */
    targetPluginId: string;
    fileName: string;
    bytes: ArrayBuffer;
  }

  interface Props {
    bundles: BundleRegistryEntry[];
    themes: ThemeRegistryEntry[];
    onSelect: (bundle: BundleRegistryEntry, pluginOverrides?: PluginOverride[]) => void | Promise<void>;
    /** Called instead of onSelect when deferZoom is true — passes launch info without zooming */
    onDeferredSelect?: (info: BundleSelectInfo) => void | Promise<void>;
    onBack?: () => void;
    /** When true, skip the zoom animation and call onDeferredSelect instead */
    deferZoom?: boolean;
  }

  let { bundles, themes, onSelect, onDeferredSelect, onBack, deferZoom = false }: Props = $props();

  let activeIndex = $state(0);
  let launching = $state(false);
  let launchRect = $state<DOMRect | null>(null);
  let containerEl = $state<HTMLElement | null>(null);

  // Detect dark mode for preview iframes
  let isDark = $state(false);
  onMount(() => {
    const stored = localStorage.getItem("diaryx-theme");
    if (stored === "dark") {
      isDark = true;
    } else if (stored !== "light") {
      isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    }
  });

  let activeBundle = $derived(bundles[activeIndex] ?? null);

  // Advanced: plugin overrides
  let showAdvanced = $state(false);
  let pluginOverrides = $state<PluginOverride[]>([]);
  let overrideFileInput = $state<HTMLInputElement | null>(null);
  let overrideTargetPluginId = $state<string | null>(null);

  function startOverrideUpload(pluginId: string) {
    overrideTargetPluginId = pluginId;
    overrideFileInput?.click();
  }

  function handleOverrideFile(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";
    if (!file.name.endsWith(".wasm")) {
      toast.error("Please select a .wasm file");
      return;
    }
    const targetId = overrideTargetPluginId ?? "__new__";
    file.arrayBuffer().then((bytes) => {
      // Replace existing override for same target, or add new
      pluginOverrides = [
        ...pluginOverrides.filter((o) => o.targetPluginId !== targetId),
        { targetPluginId: targetId, fileName: file.name, bytes },
      ];
    });
    overrideTargetPluginId = null;
  }

  function removeOverride(targetId: string) {
    pluginOverrides = pluginOverrides.filter((o) => o.targetPluginId !== targetId);
  }

  function prev() {
    if (activeIndex > 0) { activeIndex--; pluginOverrides = []; showAdvanced = false; }
  }

  function next() {
    if (activeIndex < bundles.length - 1) { activeIndex++; pluginOverrides = []; showAdvanced = false; }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowLeft") prev();
    else if (e.key === "ArrowRight") next();
    else if (e.key === "Enter" && activeBundle) handleSelect();
  }

  async function handleSelect() {
    if (!activeBundle || launching) return;

    if (deferZoom && onDeferredSelect) {
      const card = containerEl?.querySelector(`[data-bundle-index="${activeIndex}"]`) as HTMLElement | null;
      await onDeferredSelect({
        bundle: activeBundle,
        launchRect: card?.getBoundingClientRect() ?? null,
        previewUrl: previewUrl(activeBundle),
        pluginOverrides: pluginOverrides.length > 0 ? pluginOverrides : undefined,
      });
      return;
    }

    // Capture the preview card's position for the zoom animation
    const card = containerEl?.querySelector(`[data-bundle-index="${activeIndex}"]`) as HTMLElement | null;
    if (card) {
      launchRect = card.getBoundingClientRect();
    }

    launching = true;

    // Let the zoom animation play, then fire the callback
    await new Promise((r) => setTimeout(r, 600));
    await onSelect(activeBundle, pluginOverrides.length > 0 ? pluginOverrides : undefined);
  }

  function previewUrl(bundle: BundleRegistryEntry): string {
    const params = new URLSearchParams({ preview: "1", bundle: bundle.id });
    if (isDark) params.set("dark", "1");
    return `/?${params}`;
  }

  /** Extract a dominant accent color from the theme for the glow effect */
  function themeAccent(bundle: BundleRegistryEntry): string {
    const theme = themes.find((t) => t.id === bundle.theme_id);
    if (!theme) return "oklch(0.6 0.15 260)";
    const palette = isDark ? theme.theme.colors.dark : theme.theme.colors.light;
    return palette.primary;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="carousel-root flex flex-col items-center justify-center h-full w-full select-none overflow-hidden"
  bind:this={containerEl}
>
  <!-- Back button -->
  {#if onBack}
    <div class="absolute top-6 left-6 z-10 fade-in" style="animation-delay: 0.1s">
      <button
        type="button"
        class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
        onclick={onBack}
      >
        <ChevronLeft class="size-4" />
        Back
      </button>
    </div>
  {/if}

  <!-- Bundle name + description -->
  {#if activeBundle}
    <div class="text-center mb-6 z-10 fade-in" style="animation-delay: 0.2s">
      {#key activeIndex}
        <h2 class="text-2xl font-bold text-foreground mb-1 slide-fade">{activeBundle.name}</h2>
        <p class="text-sm text-muted-foreground slide-fade" style="animation-delay: 0.05s">{activeBundle.summary}</p>
      {/key}
    </div>
  {/if}

  <!-- Carousel viewport -->
  <div class="relative w-full max-w-5xl mx-auto flex items-center justify-center fade-in" style="animation-delay: 0.3s">
    <!-- Left arrow -->
    <button
      type="button"
      class="absolute left-2 md:left-4 z-10 p-2 rounded-full bg-background/80 border border-border shadow-lg backdrop-blur
        text-foreground hover:bg-secondary transition-all disabled:opacity-30 disabled:cursor-default"
      disabled={activeIndex === 0}
      onclick={prev}
    >
      <ChevronLeft class="size-5" />
    </button>

    <!-- Preview cards -->
    <div class="relative w-full overflow-hidden preview-viewport">
      {#each bundles as bundle, i (bundle.id)}
        {@const offset = i - activeIndex}
        {@const isActive = i === activeIndex}
        <div
          class="preview-card absolute top-0 left-1/2 transition-all duration-500 ease-out"
          class:pointer-events-none={!isActive}
          data-bundle-index={i}
          style="
            --offset: {offset};
            transform: translateX(calc(-50% + {offset * 110}%)) scale({isActive ? 1 : 0.75});
            opacity: {Math.abs(offset) > 1 ? 0 : isActive ? 1 : 0.5};
            z-index: {isActive ? 10 : 5 - Math.abs(offset)};
            filter: {isActive ? 'none' : 'brightness(0.7)'};
          "
        >
          <!-- Glow effect behind active card -->
          {#if isActive}
            <div
              class="absolute -inset-4 rounded-2xl blur-2xl opacity-30 transition-opacity duration-500"
              style="background: {themeAccent(bundle)}"
            ></div>
          {/if}

          <!-- Preview container -->
          <div
            class="preview-container relative rounded-xl overflow-hidden border-2 shadow-2xl transition-colors duration-300
              {isActive ? 'border-primary/50' : 'border-border/50'}"
          >
            <!-- Scaled iframe -->
            <iframe
              src={previewUrl(bundle)}
              title="Preview: {bundle.name}"
              class="preview-iframe"
              loading={Math.abs(offset) <= 1 ? "eager" : "lazy"}
              tabindex="-1"
            ></iframe>
          </div>
        </div>
      {/each}
    </div>

    <!-- Right arrow -->
    <button
      type="button"
      class="absolute right-2 md:right-4 z-10 p-2 rounded-full bg-background/80 border border-border shadow-lg backdrop-blur
        text-foreground hover:bg-secondary transition-all disabled:opacity-30 disabled:cursor-default"
      disabled={activeIndex === bundles.length - 1}
      onclick={next}
    >
      <ChevronRight class="size-5" />
    </button>
  </div>

  <!-- Dot indicators -->
  <div class="flex gap-2 mt-6 z-10 fade-in" style="animation-delay: 0.4s">
    {#each bundles as _, i}
      <button
        type="button"
        class="size-2 rounded-full transition-all duration-300
          {i === activeIndex ? 'bg-primary scale-125' : 'bg-muted-foreground/30 hover:bg-muted-foreground/50'}"
        aria-label="Go to bundle {i + 1}"
        onclick={() => { activeIndex = i; }}
      ></button>
    {/each}
  </div>

  <!-- Select button -->
  <div class="mt-6 z-10 fade-in" style="animation-delay: 0.5s">
    <Button
      class="get-started-btn px-8"
      disabled={launching}
      onclick={handleSelect}
    >
      {#if launching}
        <Loader2 class="size-4 animate-spin mr-2" />
        Setting up…
      {:else if activeBundle}
        Start with {activeBundle.name}
      {:else}
        Get Started
      {/if}
    </Button>
  </div>

  <!-- Plugin count hint + advanced toggle -->
  {#if activeBundle}
    <div class="mt-3 z-10 flex flex-col items-center">
      <button
        type="button"
        class="inline-flex items-center gap-1 text-xs text-muted-foreground/60 hover:text-muted-foreground transition-colors"
        onclick={() => { showAdvanced = !showAdvanced; }}
      >
        {#if activeBundle.plugins.length > 0}
          {activeBundle.plugins.length} plugin{activeBundle.plugins.length === 1 ? '' : 's'} included
        {:else}
          No plugins
        {/if}
        <ChevronDown class="size-3 transition-transform {showAdvanced ? 'rotate-180' : ''}" />
      </button>

      {#if showAdvanced && activeBundle}
        <div class="mt-3 w-full max-w-xs space-y-2 text-xs fade-in">
          <input
            type="file"
            accept=".wasm"
            class="hidden"
            bind:this={overrideFileInput}
            onchange={handleOverrideFile}
          />

          {#each activeBundle.plugins as plugin (plugin.plugin_id)}
            {@const override = pluginOverrides.find((o) => o.targetPluginId === plugin.plugin_id)}
            <div class="flex items-center gap-2 rounded-md border border-border px-3 py-2 bg-background/60">
              <span class="flex-1 truncate text-muted-foreground">{plugin.plugin_id}</span>
              {#if override}
                <span class="truncate text-primary max-w-[8rem]" title={override.fileName}>{override.fileName}</span>
                <button
                  type="button"
                  class="text-muted-foreground hover:text-foreground"
                  onclick={() => removeOverride(plugin.plugin_id)}
                >
                  <X class="size-3" />
                </button>
              {:else}
                <button
                  type="button"
                  class="inline-flex items-center gap-1 text-muted-foreground hover:text-foreground transition-colors"
                  onclick={() => startOverrideUpload(plugin.plugin_id)}
                >
                  <Upload class="size-3" />
                  Replace
                </button>
              {/if}
            </div>
          {/each}

          <!-- Add a plugin not in the bundle -->
          {#if pluginOverrides.find((o) => o.targetPluginId === "__new__")}
            {@const newOverride = pluginOverrides.find((o) => o.targetPluginId === "__new__")!}
            <div class="flex items-center gap-2 rounded-md border border-dashed border-primary/50 px-3 py-2 bg-background/60">
              <span class="flex-1 truncate text-primary" title={newOverride.fileName}>{newOverride.fileName}</span>
              <button
                type="button"
                class="text-muted-foreground hover:text-foreground"
                onclick={() => removeOverride("__new__")}
              >
                <X class="size-3" />
              </button>
            </div>
          {:else}
            <button
              type="button"
              class="flex items-center gap-2 w-full rounded-md border border-dashed border-border px-3 py-2 text-muted-foreground hover:text-foreground hover:border-foreground/30 transition-colors"
              onclick={() => startOverrideUpload("__new__")}
            >
              <Upload class="size-3" />
              Add plugin from file
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- Zoom overlay for launch animation -->
{#if launching && launchRect}
  <div class="launch-overlay" style="
    --start-x: {launchRect.left}px;
    --start-y: {launchRect.top}px;
    --start-w: {launchRect.width}px;
    --start-h: {launchRect.height}px;
  ">
    <div class="launch-zoom">
      <iframe
        src={activeBundle ? previewUrl(activeBundle) : ''}
        title="Launching"
        class="w-full h-full border-0"
      ></iframe>
    </div>
  </div>
{/if}

<style>
  .carousel-root {
    padding-top: calc(env(safe-area-inset-top) + var(--titlebar-area-height, 0px) + 1rem);
    padding-bottom: calc(env(safe-area-inset-bottom) + 1rem);
  }

  .preview-viewport {
    height: min(420px, 56vw);
  }

  .preview-container {
    width: min(640px, calc(100vw - 6rem));
    aspect-ratio: 16 / 10;
  }

  .preview-iframe {
    border: 0;
    width: 200%;
    height: 200%;
    transform: scale(0.5);
    transform-origin: top left;
    pointer-events: none;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
  }

  @keyframes slideFade {
    from { opacity: 0; transform: translateY(6px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .fade-in { animation: fadeIn 0.4s ease-out backwards; }
  .slide-fade { animation: slideFade 0.3s ease-out; }

  /* ---- Launch zoom animation ---- */

  .launch-overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: transparent;
    animation: overlayFadeIn 0.15s ease-out forwards;
  }

  @keyframes overlayFadeIn {
    from { background: transparent; }
    to { background: transparent; }
  }

  .launch-zoom {
    position: absolute;
    border-radius: 12px;
    overflow: hidden;
    /* Start at the card's position */
    left: var(--start-x);
    top: var(--start-y);
    width: var(--start-w);
    height: var(--start-h);
    animation: zoomToFull 0.6s cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  @keyframes zoomToFull {
    0% {
      left: var(--start-x);
      top: var(--start-y);
      width: var(--start-w);
      height: var(--start-h);
      border-radius: 12px;
    }
    100% {
      left: 0;
      top: 0;
      width: 100vw;
      height: 100vh;
      border-radius: 0;
    }
  }

  .launch-zoom iframe {
    width: 100%;
    height: 100%;
    transform: none;
    pointer-events: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .fade-in, .slide-fade { animation: none !important; }
    .launch-zoom { animation: none !important; }
    .preview-card { transition: none !important; }
  }

  :global(.get-started-btn) {
    transition: transform 0.2s ease-out, box-shadow 0.2s ease-out;
  }

  :global(.get-started-btn:hover) {
    transform: scale(1.02);
    box-shadow: 0 4px 20px color-mix(in oklch, var(--primary) 35%, transparent);
  }
</style>
