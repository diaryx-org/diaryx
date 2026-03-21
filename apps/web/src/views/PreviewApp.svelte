<script lang="ts">
  /**
   * PreviewApp — themed workspace mockup for onboarding carousel.
   *
   * Mounted instead of App when `?preview` is in the URL. Renders a static
   * workspace layout that closely mirrors the real app using the bundle's
   * theme colors. No backend, no plugins, no WASM — just themed HTML.
   */
  import { onMount } from "svelte";
  import { fetchBundleRegistry } from "$lib/marketplace/bundleRegistry";
  import { fetchThemeRegistry } from "$lib/marketplace/themeRegistry";
  import type { BundleRegistryEntry, ThemeRegistryEntry } from "$lib/marketplace/types";
  import type { ThemeColorPalette } from "$lib/stores/appearance.types";
  import { FONT_FAMILY_MAP, CONTENT_WIDTH_MAP } from "$lib/stores/appearance.types";

  interface Props {
    bundleId: string;
    darkMode?: boolean;
  }

  let { bundleId, darkMode = false }: Props = $props();

  let loaded = $state(false);

  onMount(async () => {
    if (darkMode) {
      document.documentElement.classList.add("dark");
    }

    try {
      const [bundleReg, themeReg] = await Promise.all([
        fetchBundleRegistry(),
        fetchThemeRegistry().catch(() => ({ themes: [] as ThemeRegistryEntry[] })),
      ]);

      const bundle = bundleReg.bundles.find((b: BundleRegistryEntry) => b.id === bundleId);
      if (!bundle) {
        loaded = true;
        return;
      }

      // Find the theme
      const themeEntry = themeReg.themes.find((t: ThemeRegistryEntry) => t.id === bundle.theme_id);
      if (themeEntry) {
        const palette: ThemeColorPalette = darkMode
          ? themeEntry.theme.colors.dark
          : themeEntry.theme.colors.light;

        // Apply theme CSS variables to :root
        const root = document.documentElement.style;
        for (const [key, value] of Object.entries(palette)) {
          root.setProperty(`--${key}`, value);
        }
      }

      // Apply typography if bundle specifies one
      const typoReg = await import("$lib/marketplace/typographyRegistry")
        .then((m) => m.fetchTypographyRegistry())
        .catch(() => null);

      if (typoReg && bundle.typography_id) {
        const typo = typoReg.typographies?.find(
          (t: any) => t.id === bundle.typography_id,
        );
        if (typo?.typography?.settings) {
          const s = { ...typo.typography.settings, ...(bundle.typography ?? {}) };
          const root = document.documentElement.style;
          if (s.fontFamily && FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]) {
            root.setProperty("--editor-font-family", FONT_FAMILY_MAP[s.fontFamily as keyof typeof FONT_FAMILY_MAP]);
          }
          if (s.baseFontSize) {
            root.setProperty("--editor-font-size", `${s.baseFontSize}px`);
          }
          if (s.lineHeight) {
            root.setProperty("--editor-line-height", String(s.lineHeight));
          }
          if (s.contentWidth && CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]) {
            root.setProperty("--editor-content-max-width", CONTENT_WIDTH_MAP[s.contentWidth as keyof typeof CONTENT_WIDTH_MAP]);
          }
        }
      }
    } catch (e) {
      console.warn("[PreviewApp] Failed to load bundle/theme:", e);
    }

    loaded = true;
  });

  // Sample sidebar tree entries
  const treeEntries = [
    { name: "My Workspace", depth: 0, expanded: true, isFolder: true, active: false },
    { name: "Welcome", depth: 1, expanded: false, isFolder: false, active: true },
    { name: "Getting Started", depth: 1, expanded: false, isFolder: false, active: false },
    { name: "Journal", depth: 1, expanded: true, isFolder: true, active: false },
    { name: "March 21", depth: 2, expanded: false, isFolder: false, active: false },
    { name: "March 20", depth: 2, expanded: false, isFolder: false, active: false },
    { name: "Ideas", depth: 1, expanded: false, isFolder: true, active: false },
    { name: "Reading List", depth: 1, expanded: false, isFolder: false, active: false },
  ];
</script>

<div
  class="preview-root h-full w-full flex bg-background text-foreground overflow-hidden transition-opacity duration-300"
  class:opacity-0={!loaded}
  class:opacity-100={loaded}
>
  <!-- ====== LEFT SIDEBAR ====== -->
  <div class="sidebar-left flex flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">

    <!-- Header -->
    <div class="flex items-center justify-between px-3 py-2.5 border-b border-sidebar-border bg-sidebar-accent">
      <div class="flex items-center gap-1.5">
        <span class="text-sm font-semibold text-sidebar-foreground">Diaryx</span>
        <span class="text-[10px] text-sidebar-foreground/40">1.4</span>
      </div>
      <!-- svelte-ignore element_invalid_self_closing_tag -->
      <svg class="size-4 text-sidebar-foreground/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="3" y="3" width="18" height="18" rx="2" />
        <line x1="9" y1="3" x2="9" y2="21" />
      </svg>
    </div>

    <!-- Tree view -->
    <div class="flex-1 overflow-hidden py-1">
      {#each treeEntries as entry}
        <div
          class="flex items-center gap-1 py-[3px] text-xs cursor-default select-none transition-colors
            {entry.active ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-sidebar-foreground/80'}"
          style="padding-left: {4 + entry.depth * 14}px; padding-right: 8px;"
        >
          <!-- Expand/collapse chevron -->
          {#if entry.isFolder}
            <!-- svelte-ignore element_invalid_self_closing_tag -->
            <svg class="size-3 shrink-0 text-sidebar-foreground/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              {#if entry.expanded}
                <polyline points="6 9 12 15 18 9" />
              {:else}
                <polyline points="9 6 15 12 9 18" />
              {/if}
            </svg>
          {:else}
            <div class="size-3 shrink-0"></div>
          {/if}

          <!-- Icon -->
          {#if entry.isFolder}
            <!-- svelte-ignore element_invalid_self_closing_tag -->
            <svg class="size-3.5 shrink-0 text-sidebar-foreground/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              {#if entry.expanded}
                <path d="M5 19a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4l2 2h9a2 2 0 0 1 2 2v1M5 19h14a2 2 0 0 0 2-2v-5a2 2 0 0 0-2-2H9a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2Z"/>
              {:else}
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
              {/if}
            </svg>
          {:else}
            <!-- svelte-ignore element_invalid_self_closing_tag -->
            <svg class="size-3.5 shrink-0 text-sidebar-foreground/50" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
              <polyline points="14 2 14 8 20 8"/>
              <line x1="16" y1="13" x2="8" y2="13"/>
              <line x1="16" y1="17" x2="8" y2="17"/>
            </svg>
          {/if}

          <span class="truncate">{entry.name}</span>
        </div>
      {/each}
    </div>

    <!-- Tab bar -->
    <div class="px-2 py-1.5 border-t border-sidebar-border">
      <div class="flex gap-0.5 bg-muted rounded-md p-0.5">
        <div class="flex-1 flex items-center justify-center gap-1 py-1 rounded text-[10px] font-medium bg-background text-foreground shadow-sm">
          <!-- svelte-ignore element_invalid_self_closing_tag -->
          <svg class="size-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
          </svg>
          Files
        </div>
        <div class="flex-1 flex items-center justify-center gap-1 py-1 rounded text-[10px] font-medium text-muted-foreground">
          <!-- svelte-ignore element_invalid_self_closing_tag -->
          <svg class="size-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="18" cy="5" r="3"/>
            <circle cx="6" cy="12" r="3"/>
            <circle cx="18" cy="19" r="3"/>
            <line x1="8.59" y1="13.51" x2="15.42" y2="17.49"/>
            <line x1="15.41" y1="6.51" x2="8.59" y2="10.49"/>
          </svg>
          Share
        </div>
        <div class="flex-1 flex items-center justify-center gap-1 py-1 rounded text-[10px] font-medium text-muted-foreground">
          <!-- svelte-ignore element_invalid_self_closing_tag -->
          <svg class="size-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
            <polyline points="12 6 12 12 16 14"/>
          </svg>
          History
        </div>
      </div>
    </div>

    <!-- Workspace selector -->
    <div class="px-2 py-1.5 border-t border-sidebar-border">
      <div class="flex items-center gap-2 px-2 py-1.5 rounded-md text-xs text-sidebar-foreground/70 cursor-default">
        <!-- svelte-ignore element_invalid_self_closing_tag -->
        <svg class="size-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="2" y="2" width="20" height="8" rx="2" ry="2"/>
          <rect x="2" y="14" width="20" height="8" rx="2" ry="2"/>
          <line x1="6" y1="6" x2="6.01" y2="6"/>
          <line x1="6" y1="18" x2="6.01" y2="18"/>
        </svg>
        <span class="truncate flex-1">My Workspace</span>
        <!-- svelte-ignore element_invalid_self_closing_tag -->
        <svg class="size-3 shrink-0 text-sidebar-foreground/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="6 9 12 15 18 9"/>
        </svg>
      </div>
    </div>

    <!-- Profile footer -->
    <div class="px-3 py-2 border-t border-sidebar-border">
      <div class="flex items-center gap-2 text-xs text-sidebar-foreground/60">
        <!-- svelte-ignore element_invalid_self_closing_tag -->
        <svg class="size-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="8" r="5"/>
          <path d="M20 21a8 8 0 1 0-16 0"/>
        </svg>
        <span class="truncate">you@example.com</span>
      </div>
    </div>
  </div>

  <!-- ====== MAIN CONTENT ====== -->
  <div class="flex-1 flex flex-col min-w-0 bg-background">

    <!-- Editor content -->
    <div class="flex-1 overflow-hidden">
      <div
        class="mx-auto px-6 py-6"
        style="
          max-width: var(--editor-content-max-width, 65ch);
          font-family: var(--editor-font-family);
          font-size: var(--editor-font-size, 16px);
          line-height: var(--editor-line-height, 1.6);
        "
      >
        <h1 class="text-2xl font-bold text-foreground mb-4">Welcome</h1>
        <p class="text-foreground/90 mb-4">
          Your personal knowledge workspace. Write, organize, and connect your thoughts
          in a single, beautiful interface.
        </p>
        <h2 class="text-lg font-semibold text-foreground mt-6 mb-3">Getting Started</h2>
        <p class="text-foreground/80 mb-3">
          Everything lives in a tree on
          the left. Create entries, nest them, and link them together.
        </p>
        <ul class="space-y-2 mb-4">
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Organize with nested entries and folders</span>
          </li>
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Rich text editing with markdown shortcuts</span>
          </li>
          <li class="flex items-start gap-2">
            <span class="mt-2 size-1.5 rounded-full bg-primary shrink-0"></span>
            <span class="text-foreground/80">Sync across all your devices</span>
          </li>
        </ul>
        <blockquote class="border-l-3 border-primary/40 pl-4 italic text-muted-foreground mb-4">
          "The best time to start writing is now."
        </blockquote>
        <p class="text-foreground/70 text-sm">
          Explore the sidebar to see how entries are organized, or dive in and start writing.
        </p>
      </div>
    </div>

    <!-- Editor footer -->
    <div class="flex items-center justify-end gap-1.5 px-3 py-1.5 border-t border-border">
      <div class="size-5 rounded flex items-center justify-center text-muted-foreground/50">
        <!-- svelte-ignore element_invalid_self_closing_tag -->
        <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
        </svg>
      </div>
      <div class="size-5 rounded flex items-center justify-center text-muted-foreground/50">
        <!-- svelte-ignore element_invalid_self_closing_tag -->
        <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"/>
        </svg>
      </div>
    </div>
  </div>

  <!-- ====== RIGHT SIDEBAR (collapsed, icon strip) ====== -->
  <div class="sidebar-right shrink-0 border-l border-border bg-sidebar flex flex-col items-center py-2.5 gap-2">
    <!-- svelte-ignore element_invalid_self_closing_tag -->
    <svg class="sidebar-icon text-muted-foreground/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
      <polyline points="14 2 14 8 20 8"/>
    </svg>
    <!-- svelte-ignore element_invalid_self_closing_tag -->
    <svg class="sidebar-icon text-muted-foreground/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="12" cy="12" r="10"/>
      <polyline points="12 6 12 12 16 14"/>
    </svg>
    <!-- svelte-ignore element_invalid_self_closing_tag -->
    <svg class="sidebar-icon text-muted-foreground/40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z"/>
    </svg>
  </div>
</div>

<style>
  .preview-root {
    color: var(--foreground);
    background: var(--background);
  }

  .sidebar-left {
    width: 210px;
    min-width: 0;
  }

  .sidebar-right {
    width: 36px;
  }

  .sidebar-icon {
    width: 16px;
    height: 16px;
  }

  /* Scale down for small viewports (carousel iframes) */
  @container (max-width: 500px) {
    .sidebar-left { width: 160px; }
  }
</style>
